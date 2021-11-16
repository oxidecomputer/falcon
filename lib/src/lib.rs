// Copyright 2021 Oxide Computer Company

mod test;
mod util;

pub mod cli;
pub mod error;
pub mod serial;

use tokio::time::{sleep, Duration};
use std::net::{
    IpAddr,
    Ipv6Addr,
    SocketAddr,
};
use std::str::FromStr;
use error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use serde::{Serialize, Deserialize};
use ron::ser::{to_string_pretty, PrettyConfig};
use slog::{debug, warn, info, error, Logger};
use slog::Drain;
use std::process::Command;
use std::collections::BTreeMap;
use futures::future::join_all;

pub struct Runner {
    /// The deployment object that describes the Falcon topology
    pub deployment: Deployment,

    /// If persistent is set to true, this deployment will not autodestruct when
    /// dropped.
    pub persistent: bool,

    log: Logger,
}

/// A Deployment is the top level Falcon object. It contains a set of nodes and
/// links that are logically namespaced under the name of the deployment. Links
/// interconnect nodes forming a network.
#[derive(Serialize, Deserialize)]
pub struct Deployment {
    /// The name of this deployment
    pub name: String,

    /// The nodes of this deployment
    pub nodes: Vec<Node>,

    /// The point to point links of this deployment interconnectiong nodes
    pub links: Vec<Link>,
}

impl Default for Deployment {
    fn default() -> Self {
        Deployment {
            name: "".to_string(),
            nodes: Vec::new(),
            links: Vec::new(),
        }
    }
}

/// A node in a falcon network.
#[derive(Serialize, Deserialize)]
pub struct Node {
    /// Name of the node
    pub name: String,
    /// Image node uses
    pub image: String,
    /// How many links the node has
    pub radix: usize,
    /// Mounted file systems
    pub mounts: Vec<Mount>,
    /// uuid of the node
    pub id: uuid::Uuid,
}

/// Directories mounted from host machine into a node.
#[derive(Serialize, Deserialize)]
pub struct Mount {
    /// Directory from host to mount.
    pub source: String,

    /// Directory in node to mount to.
    pub destination: String,
}

/// Node references are passed back to clients when nodes are created. These are
/// an opaque handle that can be used in conjunction with various methods
/// provided by the Deployment implementation.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct NodeRef {
    /// The index of the referenced node in `Deployment::nodes`
    index: usize,
}

/// Links connect nodes through a pair of Endpoints. Links are strictly point to
/// point. They are meant to represent a single cable between machines. The only
/// future exception to this may be for breakout cables that have a 1 to N
/// fanout.
#[derive(Serialize, Deserialize)]
pub struct Link {
    pub endpoints: [Endpoint; 2],
}

/// Endpoints are owned by a Link and reference nodes through a references.
#[derive(Serialize, Deserialize)]
pub struct Endpoint {
    /// The node this endpiont is attached to
    node: NodeRef,

    /// The link index within the referenced node e.g., if this is the 3rd link
    /// in the referenzed node index=2.
    index: usize,
}

/// Opaque handle to a link. Used by clients to perform API functions on
/// links owned by a Deployment.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct LinkRef {
    /// The index of the referenced link in `Deployment::links`
    _index: usize,
}

impl Runner {
    pub fn new(name: &str) -> Self {
        namecheck!(name, "deployment");

        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_envlogger::new(drain).fuse();
        let drain = slog_async::Async::new(drain).build().fuse();

        Runner {
            deployment: Deployment::new(name),
            log: slog::Logger::root(drain, slog::o!()),
            persistent: false,
        }

    }

    /// Create a new node within this deployment with the given name. Names must
    /// conform to [A-Za-z]?[A-Za-z0-9_]*
    pub fn node(&mut self, name: &str, image: &str) -> NodeRef {
        namecheck!(name, "node");

        let id = uuid::Uuid::new_v4();

        let r = NodeRef {
            index: self.deployment.nodes.len(),
        };
        let n = Node {
            name: String::from(name),
            image: String::from(image),
            radix: 0,
            mounts: Vec::new(),
            id,
        };
        self.deployment.nodes.push(n);
        r
    }

    /// Create a new link within this deployment between the referenced nodes.
    pub fn link(&mut self, a: NodeRef, b: NodeRef) -> LinkRef {
        let r = LinkRef {
            _index: self.deployment.links.len(),
        };
        let l = Link {
            endpoints: [
                Endpoint {
                    node: a,
                    index: self.deployment.nodes[a.index].radix,
                },
                Endpoint {
                    node: b,
                    index: self.deployment.nodes[b.index].radix,
                },
            ],
        };
        self.deployment.links.push(l);
        self.deployment.nodes[a.index].radix += 1;
        self.deployment.nodes[b.index].radix += 1;
        r
    }

    pub fn mount(
        &mut self,
        src: impl AsRef<str>,
        dst: impl AsRef<str>,
        n: NodeRef,
    ) -> Result<(), Error> {
        let pb = PathBuf::from(src.as_ref());
        let cpath = fs::canonicalize(&pb)?;
        let cpath_str = cpath
            .to_str()
            .ok_or(Error::PathError(format!("bad path: {}", src.as_ref())))?;

        self.deployment.nodes[n.index].mounts.push(Mount {
            source: cpath_str.to_string(),
            destination: dst.as_ref().to_string(),
        });

        Ok(())
    }

    /// Launch the deployment. This will clone the necessary image zvols, create
    /// the propolis VM instances, create the point to point network interfaces,
    /// set up the serial console for each VM and, run any user defined exec
    /// statements.
    pub async fn launch(&self) -> Result<(), Error> {

        self.preflight()?;
        match self.do_launch().await {
            Ok(()) => Ok(()),
            Err(e) => {
                error!(self.log, "launch failed: {}", e);
                Err(e)
            }
        }
    }

    pub fn preflight(&self) -> Result<(), Error> {

        // ensure falcon working dir
        fs::create_dir_all(".falcon")?;

        // write falcon config
        let pretty = PrettyConfig::new()
            .separate_tuple_members(true);
        let out = format!("{}\n", to_string_pretty(&self.deployment, pretty)?);
        fs::write(".falcon/topology.ron", out)?;

        for n in self.deployment.nodes.iter() {
            n.preflight(&self)?;
        }

        Ok(())
    }

    async fn do_launch(&self) -> Result<(), Error> {

        info!(self.log, "creating links");
        for l in self.deployment.links.iter() {
            l.create(&self)?;
        }

        info!(self.log, "creating nodes");
        //TODO available port finder
        let mut port = 10000;

        let mut fs = Vec::new();
        for n in self.deployment.nodes.iter() {
            fs.push(n.launch(&self, port));
            port += 1;
        }
        for x in join_all(fs).await {
            x?;
        }

        Ok(())
    }

    /// Tear down all the nodes, followed by the links and the ZFS pool
    // TODO in parallel
    pub fn destroy(&self) -> Result<(), Error> {

        debug!(self.log, "destroying nodes");
        for n in self.deployment.nodes.iter() {
            n.destroy(&self)?;
        }

        debug!(self.log, "destroying links");
        for l in self.deployment.links.iter() {
            l.destroy(&self)?;
        }

        // Destroy images
        debug!(self.log, "destroying images");
        let img = format!("rpool/falcon/topo/{}", self.deployment.name);
        Command::new("zfs").args(&["destroy", "-r", img.as_ref()]).output()?;

        // Destroy workspace
        debug!(self.log, "destroying workspace");
        fs::remove_dir_all(".falcon")?;

        Ok(())
    }

    /// Run a command synchronously in the vm.
    pub async fn exec(&self, n: NodeRef, cmd: &str) -> Result<String, Error> {

        let name = self.deployment.nodes[n.index].name.clone();
        self.do_exec(&name, cmd).await
    }

    async fn do_exec(&self, name: &str, cmd: &str) -> Result<String, Error> {

        let id = match fs::read_to_string(format!(".falcon/{}.uuid", name)) {
            Ok(u) => u,
            Err(e) => {
                return Err(Error::NotFound(
                        format!("propolis uuid for {}: {}", name, e)
                ));
            }
        };

        let port = match fs::read_to_string(format!(".falcon/{}.port", name)) {
            Ok(p) => {
                u16::from_str_radix(p.as_str(), 10)?
            },
            Err(e) => {
                return Err(Error::NotFound(
                        format!("get propolis port for {}: {}", name, e)
                ));
            }
        };


        let addr = SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                port,
        );

        let mut sc = serial::SerialCommander::new(addr, id, self.log.clone());
        let mut ws = sc.connect().await?;

        // if we are here, we are already logged in on the serial port
        Ok(sc.exec(&mut ws, cmd.to_string()).await?)
    }

    /// Run a command asynchronously in the node.
    pub fn spawn(
        &self,
        n: NodeRef,
        cmd: &str,
    ) -> Receiver<Result<String, Error>> {
        let (_tx, rx): (
            Sender<Result<String, Error>>,
            Receiver<Result<String, Error>>,
        ) = mpsc::channel();

        let _name = self.deployment.nodes[n.index].node_name(&self.deployment);
        let _cmd = cmd.to_string();

        thread::spawn(move || {
            //TODO
        });

        rx
    }

}

impl Deployment {
    /// Create a new deployment with the given name. Names must conform to
    /// [A-Za-z]?[A-Za-z0-9_]*
    pub fn new(name: &str) -> Self {
        namecheck!(name, "deployment");

        Deployment {
            name: String::from(name),
            nodes: Vec::new(),
            links: Vec::new(),
        }
    }

    fn simnet_link_name(&self, e: &Endpoint) -> String {
        format!(
            "{}_{}_sim{}",
            self.name, self.nodes[e.node.index].name, e.index,
        )
    }

    fn vnic_link_name(&self, e: &Endpoint) -> String {
        format!(
            "{}_{}_vnic{}",
            self.name, self.nodes[e.node.index].name, e.index,
        )
    }

}

impl Drop for Runner {
    fn drop(&mut self) {
        if !self.persistent {
            match self.destroy() {
                Ok(()) => {}
                Err(e) => error!(self.log, "cleanup failed: {}", e),
            }
        }
    }
}

impl Node {
    fn preflight(&self, r: &Runner) -> Result<(), Error> {

        //Clone base image

        //TODO incorporate version into img
        let source = format!("rpool/falcon/img/{}@1.0", self.image);
        let dest = format!(
            "rpool/falcon/topo/{}/{}", r.deployment.name, self.name);

        Command::new("zfs")
            .args(&[
                "clone",
                "-p",
                source.as_ref(),
                dest.as_ref(),
            ]).output()?;

        // create propolis config

        let mut devices = BTreeMap::new();
        let mut device_options = BTreeMap::new();

        let mut block_devs = BTreeMap::new();
        let mut blockdev_options = BTreeMap::new();

        // main disk

        let zvol = format!(
            "/dev/zvol/dsk/rpool/falcon/topo/{}/{}",
            r.deployment.name,
            self.name,
        );
        device_options.insert(
            "block_dev".to_string(), 
            toml::Value::String("main_disk".to_string()),
        );
        device_options.insert(
            "pci-path".to_string(),
            toml::Value::String("0.4.0".to_string()),
        );
        devices.insert(
            "block0".to_string(), 
            propolis_server::config::Device{
                driver: "pci-virtio-block".to_string(),
                options: device_options,
            },
        );
        blockdev_options.insert(
            "path".to_string(),
            toml::Value::String(zvol),
        );
        block_devs.insert(
            "main_disk".to_string(),
            propolis_server::config::BlockDevice{
                bdtype: "file".to_string(),
                options: blockdev_options,
            },
        );

        // mounts
        for (i, m) in self.mounts.iter().enumerate() {

            let mut opts = BTreeMap::new();
            opts.insert("source".to_string(), m.source.clone().into());
            opts.insert("target".to_string(), m.destination.clone().into());
            opts.insert("pci-path".to_string(), "0.5.0".into());

            devices.insert(
                format!("fs{}", i),
                propolis_server::config::Device{
                    driver: "pci-virtio-9p".to_string(),
                    options: opts,
                },
            );

        }


        // network interfaces
        let d = &r.deployment;

        let mut links: Vec<String> = Vec::new();
        let mut i = 0;
        let mut p = 6;
        for l in d.links.iter() {
            for e in l.endpoints.iter() {
                if d.nodes[e.node.index].name == self.name {
                    links.push(d.vnic_link_name(e));
                    let mut opts = BTreeMap::new();
                    opts.insert(
                        "vnic".to_string(),
                        toml::Value::String(d.vnic_link_name(e)),
                    );
                    opts.insert(
                        "pci-path".to_string(),
                        toml::Value::String(format!("0.{}.0", p)),
                    );
                    devices.insert(
                        format!("net{}", i),
                        propolis_server::config::Device{
                            driver: "pci-virtio-viona".to_string(),
                            options: opts,
                        },
                    );
                    i += 1;
                    p += 1;
                }
            }
        }
            
        
        // write propolis instance config to .falcon/<node-name>.toml
        let propolis_config = propolis_server::config::Config::new(
            PathBuf::from("/var/ovmf/OVMF_CODE.fd"), //TODO needs to come from somewhere
            devices,
            block_devs,
        );

        let config_toml = toml::to_string(&propolis_config)?;
        fs::write(
            format!(".falcon/{}.toml", self.name),
            config_toml,
        )?;

        Ok(())

    }

    async fn launch(&self, r: &Runner, port: u32) -> Result<(), Error> {

        // launch propolis-server

        fs::write(format!(".falcon/{}.port", self.name),  port.to_string())?;

        let stdout  = fs::File::create(format!(".falcon/{}.out", self.name))?;
        let stderr  = fs::File::create(format!(".falcon/{}.err", self.name))?;
        let config = format!(".falcon/{}.toml", self.name);
        let sockaddr = format!("[::]:{}", port);
        let mut cmd = Command::new("propolis-server");
        cmd.args(&["run", config.as_ref(), sockaddr.as_ref()])
            .stdout(stdout)
            .stderr(stderr);
        let child = cmd.spawn()?;

        fs::write(format!(
                ".falcon/{}.pid", self.name), child.id().to_string())?;

        info!(r.log,
            "launched instance {} with pid {} on port {}",
            self.name,
            child.id(),
            port,
        );

        let sockaddr = format!("[::1]:{}", port);

        // create vm instance
        let client = propolis_client::Client::new(
            SocketAddr::from_str(sockaddr.as_ref())?,
            r.log.clone(),
        );


        let id = uuid::Uuid::new_v4();
        fs::write(format!(
                ".falcon/{}.uuid", self.name), id.to_string())?;

        let properties = propolis_client::api::InstanceProperties {
            id,
            name: self.name.clone(),
            description: "a falcon vm".to_string(),
            image_id: uuid::Uuid::default(),
            bootrom_id: uuid::Uuid::default(),
            memory: 1024, //TODO hardcode
            vcpus: 1, //TODO hardcode
        };
        let req = propolis_client::api::InstanceEnsureRequest {
            properties,
            nics: Vec::new(),
        };

        // we just launched the instance, so wait for it to become ready
        let mut success = false;
        for _ in 0..30 {
            match client.instance_ensure(&req).await {
                Ok(_) => {
                    success = true;
                    break;
                }
                Err(_) => {
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }
        }
        if !success {
            client.instance_ensure(&req).await?;
        }

        // run vm instance
        client.instance_state_put(
            id,
            propolis_client::api::InstanceStateRequested::Run,
        ).await?;

        let ws_sockaddr = format!("[::1]:{}", port);

        // login to serial console
        let mut sc = serial::SerialCommander::new(
            SocketAddr::from_str(ws_sockaddr.as_ref())?,
            id.to_string(),
            r.log.clone(),
        );
        sc.start().await?;

        // setup mounts
        // TODO this will only work as expected for one mount.
        for mount in &self.mounts {
            debug!(r.log, "mouting {}", mount.destination);
            r.do_exec(&self.name, "p9kp load-driver").await?;
            let cmd = format!(
                "mkdir -p {dst}; cd {dst}; p9kp pull", dst=mount.destination);
            r.do_exec(&self.name, &cmd).await?;
            r.do_exec(&self.name, "cd").await?;
        }


        Ok(())
    }

    fn node_name(&self, d: &Deployment) -> String {
        format!("{}_{}", d.name, self.name)
    }

    fn destroy(&self, r: &Runner) -> Result<(), Error> {

        // get propolis pid
        let pid = match fs::read_to_string(format!(".falcon/{}.pid", self.name)) {
            Ok(pid) => {
                match i32::from_str_radix(pid.as_ref(), 10) {
                    Ok(pid) => pid,
                    Err(e) => {
                        warn!(r.log, "parse propolis pid for {}: {}", self.name, e);
                        return Ok(());
                    }
                }
            }
            Err(e) =>  {
                warn!(r.log, "get propolis pid for {}: {}", self.name, e);
                return Ok(());
            }
        };

        // kill propolis instance
        unsafe { libc::kill(pid, libc::SIGKILL); }

        // get instance uuid
        let uuid = match fs::read_to_string(format!(".falcon/{}.uuid", self.name)) {
            Ok(u) => u,
            Err(e) => {
                warn!(r.log, "get propolis uuid for {}: {}", self.name, e);
                return Ok(());
            }
        };

        // destroy bhyve vm
        let vm_arg = format!("--vm={}", uuid);
        match Command::new("bhyvectl").args(&["--destroy", vm_arg.as_ref()]).output() {
            Ok(_) => {}
            Err(e) => {
                warn!(r.log, "delete bhyve vm for {}: {}", self.name, e);
                return Ok(());
            }
        }

        Ok(())
    }
}

impl Link {
    fn create(&self, r: &Runner) -> Result<(), Error> {

        let d = &r.deployment;

        // create interfaces
        for e in self.endpoints.iter() {
            let slink = d.simnet_link_name(e);
            let vlink = d.vnic_link_name(e);

            let slink_h = netadm_sys::LinkHandle::Name(slink.clone());
            let vlink_h = netadm_sys::LinkHandle::Name(vlink.clone());

            // if dangling links exists, remove them
            debug!(r.log, "destroying link {}", &vlink);
            netadm_sys::delete_link(&vlink_h, netadm_sys::LinkFlags::Active)?;
            debug!(r.log, "destroying link {}", &slink);
            netadm_sys::delete_link(&slink_h, netadm_sys::LinkFlags::Active)?;

            debug!(r.log, "creating simnet link '{}'", &slink);
            netadm_sys::create_simnet_link(
                &slink, netadm_sys::LinkFlags::Active)?;

            debug!(r.log, "creating vnic link '{}'", &vlink);
            netadm_sys::create_vnic_link(
                &vlink, &slink_h, netadm_sys::LinkFlags::Active)?;

            debug!(r.log, "link pair created");
        }

        // make point to point connection beteween interfaces
        let slink0 = d.simnet_link_name(&self.endpoints[0]);
        let slink1 = d.simnet_link_name(&self.endpoints[1]);
        let slink0_h = netadm_sys::LinkHandle::Name(slink0);
        let slink1_h = netadm_sys::LinkHandle::Name(slink1);
        netadm_sys::connect_simnet_peers(&slink0_h, &slink1_h)?;

        Ok(())
    }

    fn destroy(&self, r: &Runner) -> Result<(), Error> {

        let d = &r.deployment;

        for e in self.endpoints.iter() {
            let slink = d.simnet_link_name(e);
            let vlink = d.vnic_link_name(e);
            let slink_h = netadm_sys::LinkHandle::Name(slink.clone());
            let vlink_h = netadm_sys::LinkHandle::Name(vlink.clone());

            debug!(r.log, "destroying link {}", &vlink);
            netadm_sys::delete_link(&vlink_h, netadm_sys::LinkFlags::Active)?;
            debug!(r.log, "destroying link {}", &slink);
            netadm_sys::delete_link(&slink_h, netadm_sys::LinkFlags::Active)?;
        }

        Ok(())
    }
}

// Copyright 2021 Oxide Computer Company

mod test;
mod util;

pub mod cli;
pub mod error;
pub mod serial;
pub mod unit;

use error::Error;
use futures::future::join_all;
use ron::ser::{to_string_pretty, PrettyConfig};
use serde::{Deserialize, Serialize};
use slog::Drain;
use slog::{debug, error, info, warn, Logger};
use std::collections::BTreeMap;
use std::convert::TryInto;
use std::fs;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use tokio::time::{sleep, Duration};

pub struct Runner {
    /// The deployment object that describes the Falcon topology
    pub deployment: Deployment,

    /// If persistent is set to true, this deployment will not autodestruct when
    /// dropped.
    pub persistent: bool,

    /// The propolis-server binary to use
    pub propolis_binary: String,

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

    pub ext_links: Vec<ExtLink>,
}

impl Default for Deployment {
    fn default() -> Self {
        Deployment {
            name: "".to_string(),
            nodes: Vec::new(),
            links: Vec::new(),
            ext_links: Vec::new(),
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
    /// how many cores to give the node
    pub cores: u8,
    /// how much memory to give the node in mb
    pub memory: u64,
}

/// Directories mounted from host machine into a node.
#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Serialize, Deserialize)]
pub struct ExtLink {
    pub endpoint: Endpoint,
    pub host_ifx: String,
}

/// Endpoint kind determines what type of device will be chosen to underpin a
/// given endpoint on a VM.
#[derive(Serialize, Deserialize, Clone)]
pub enum EndpointKind {
    /// Use a bhyve/viona kernel device. This is the default.
    Viona,

    /// Use a Sidecar multiplexing device. If you are unsure, this is not what
    /// you want. The usize parameter indicates radix of the connected Sidecar
    /// device.
    Sidemux(usize),
}

/// Endpoints are owned by a Link and reference nodes through a references.
#[derive(Serialize, Deserialize, Clone)]
pub struct Endpoint {
    /// The node this endpiont is attached to
    node: NodeRef,

    /// The link index within the referenced node e.g., if this is the 3rd link
    /// in the referenzed node index=2.
    index: usize,

    /// What kind of virtual device this endpoint will be realized as.
    kind: EndpointKind,
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

        match std::env::var("RUST_LOG") {
            Ok(s) => {
                if s.is_empty() {
                    std::env::set_var("RUST_LOG", "info");
                }
            }
            _ => {
                std::env::set_var("RUST_LOG", "info");
            }
        }

        let decorator = slog_term::TermDecorator::new().build();
        let drain = slog_term::FullFormat::new(decorator).build().fuse();
        let drain = slog_envlogger::new(drain).fuse();
        let drain = slog_async::Async::new(drain).build().fuse();

        Runner {
            deployment: Deployment::new(name),
            log: slog::Logger::root(drain, slog::o!()),
            persistent: false,
            propolis_binary: "propolis-server".into(),
        }
    }

    /// Create a new node within this deployment with the given name. Names must
    /// conform to `[A-Za-z]?[A-Za-z0-9_]*`
    pub fn node(&mut self, name: &str, image: &str, cores: u8, memory: u64) -> NodeRef {
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
            cores,
            memory,
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
                    kind: EndpointKind::Viona,
                },
                Endpoint {
                    node: b,
                    index: self.deployment.nodes[b.index].radix,
                    kind: EndpointKind::Viona,
                },
            ],
        };
        self.deployment.links.push(l);
        self.deployment.nodes[a.index].radix += 1;
        self.deployment.nodes[b.index].radix += 1;
        r
    }

    /// Create a sidecar controller link with the provided radix.
    ///
    /// The sidecar node will get a regular bhyve/viona endpoint. The controller
    /// node will get a sidemux device with the provided radix.
    pub fn sidecar_link(&mut self, sidecar: NodeRef, controller: NodeRef, radix: usize) -> LinkRef {
        let r = LinkRef {
            _index: self.deployment.links.len(),
        };
        let l = Link {
            endpoints: [
                Endpoint {
                    node: sidecar,
                    index: self.deployment.nodes[sidecar.index].radix,
                    kind: EndpointKind::Viona,
                },
                Endpoint {
                    node: controller,
                    index: self.deployment.nodes[controller.index].radix,
                    kind: EndpointKind::Sidemux(radix),
                },
            ],
        };
        self.deployment.links.push(l);
        self.deployment.nodes[sidecar.index].radix += 1;
        self.deployment.nodes[controller.index].radix += radix;
        r
    }

    /// Create an external link attached to `host_ifx`.
    pub fn ext_link(&mut self, host_ifx: impl AsRef<str>, n: NodeRef) {
        let endpoint = Endpoint {
            node: n,
            index: self.deployment.nodes[n.index].radix,
            kind: EndpointKind::Viona,
        };
        let host_ifx = host_ifx.as_ref().into();
        self.deployment
            .ext_links
            .push(ExtLink { endpoint, host_ifx });
        self.deployment.nodes[n.index].radix += 1;
    }

    /// Provide the host folder `src` as a p9fs mount to the guest with the tag
    /// `dst`.
    pub fn mount(
        &mut self,
        src: impl AsRef<str>,
        dst: impl AsRef<str>,
        n: NodeRef,
    ) -> Result<(), Error> {
        let pb = PathBuf::from(src.as_ref());
        let cpath = fs::canonicalize(&pb)
            .map_err(|e| Error::PathError(format!("{}: {}", src.as_ref(), e)))?;
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

    fn preflight(&self) -> Result<(), Error> {
        // Verify all required executables are discoverable.
        let out = Command::new(&self.propolis_binary).args(&["-V"]).output();
        if out.is_err() {
            return Err(Error::Exec(format!(
                "failed to find {} on PATH",
                &self.propolis_binary
            )));
        }

        // ensure falcon working dir
        fs::create_dir_all(".falcon")?;

        // write falcon config
        let pretty = PrettyConfig::new().separate_tuple_members(true);
        let out = format!("{}\n", to_string_pretty(&self.deployment, pretty)?);
        fs::write(".falcon/topology.ron", out)?;

        for n in self.deployment.nodes.iter() {
            n.preflight(&self)?;
        }

        Ok(())
    }

    async fn net_launch(&self) -> Result<(), Error> {
        info!(self.log, "creating links");
        for l in self.deployment.links.iter() {
            l.create(&self)?;
        }

        info!(self.log, "creating external links");
        for l in self.deployment.ext_links.iter() {
            l.create(&self)?;
        }

        Ok(())
    }

    async fn do_launch(&self) -> Result<(), Error> {
        self.net_launch().await?;

        info!(self.log, "creating nodes");

        let mut fs = Vec::new();
        for n in self.deployment.nodes.iter() {
            let port = match portpicker::pick_unused_port() {
                Some(p) => p,
                None => return Err(Error::NoPorts),
            };
            fs.push(n.launch(&self, port as u32));
        }
        for x in join_all(fs).await {
            x?;
        }

        Ok(())
    }

    pub fn net_destroy(&self) -> Result<(), Error> {
        info!(self.log, "destroying links");
        for l in self.deployment.links.iter() {
            l.destroy(&self)?;
        }

        info!(self.log, "destroying external links");
        for l in self.deployment.ext_links.iter() {
            l.destroy(&self)?;
        }
        Ok(())
    }

    /// Tear down all the nodes, followed by the links and the ZFS pool
    // TODO in parallel
    pub fn destroy(&self) -> Result<(), Error> {
        info!(self.log, "destroying nodes");
        for n in self.deployment.nodes.iter() {
            n.destroy(&self)?;
        }

        self.net_destroy()?;

        // Destroy images
        info!(self.log, "destroying images");
        let img = format!("rpool/falcon/topo/{}", self.deployment.name);
        Command::new("zfs")
            .args(&["destroy", "-r", img.as_ref()])
            .output()?;

        // Destroy workspace
        info!(self.log, "destroying workspace");
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
                return Err(Error::NotFound(format!(
                    "propolis uuid for {}: {}",
                    name, e
                )));
            }
        };

        let port = match fs::read_to_string(format!(".falcon/{}.port", name)) {
            Ok(p) => u16::from_str_radix(p.as_str(), 10)?,
            Err(e) => {
                return Err(Error::NotFound(format!(
                    "get propolis port for {}: {}",
                    name, e
                )));
            }
        };

        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), port);

        let mut sc = serial::SerialCommander::new(addr, id, self.log.clone());
        let mut ws = sc.connect().await?;

        // if we are here, we are already logged in on the serial port
        Ok(sc.exec(&mut ws, cmd.to_string()).await?)
    }

    /// Run a command asynchronously in the node.
    pub fn spawn(&self, n: NodeRef, cmd: &str) -> Receiver<Result<String, Error>> {
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
            ext_links: Vec::new(),
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
        let source = format!("rpool/falcon/img/{}@base", self.image);
        let dest = format!("rpool/falcon/topo/{}/{}", r.deployment.name, self.name);

        let out = Command::new("zfs")
            .args(&["clone", "-p", source.as_ref(), dest.as_ref()])
            .output()?;

        if !out.status.success() {
            return Err(Error::Zfs(String::from_utf8(out.stderr)?));
        }

        // create propolis config

        let mut devices = BTreeMap::new();
        let mut device_options = BTreeMap::new();

        let mut block_devs = BTreeMap::new();
        let mut blockdev_options = BTreeMap::new();

        // main disk

        let zvol = format!(
            "/dev/zvol/dsk/rpool/falcon/topo/{}/{}",
            r.deployment.name, self.name,
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
            propolis_server::config::Device {
                driver: "pci-virtio-block".to_string(),
                options: device_options,
            },
        );
        blockdev_options.insert("path".to_string(), toml::Value::String(zvol));
        block_devs.insert(
            "main_disk".to_string(),
            propolis_server::config::BlockDevice {
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
                propolis_server::config::Device {
                    driver: "pci-virtio-9p".to_string(),
                    options: opts,
                },
            );
        }

        // network interfaces
        let d = &r.deployment;

        //let mut links: Vec<String> = Vec::new();
        let mut viona_index = 0;
        let mut sidemux_index = 0;
        let mut pci_index = 6;

        let mut endpoints = Vec::new();
        for l in &d.links {
            endpoints.extend_from_slice(&l.endpoints);
        }
        for l in &d.ext_links {
            endpoints.push(l.endpoint.clone());
        }

        for e in &endpoints {
            if d.nodes[e.node.index].name == self.name {
                match e.kind {
                    EndpointKind::Viona => {
                        //links.push(d.vnic_link_name(e));
                        let mut opts = BTreeMap::new();
                        opts.insert("vnic".to_string(), toml::Value::String(d.vnic_link_name(e)));
                        opts.insert(
                            "pci-path".to_string(),
                            toml::Value::String(format!("0.{}.0", pci_index)),
                        );
                        devices.insert(
                            format!("net{}", viona_index),
                            propolis_server::config::Device {
                                driver: "pci-virtio-viona".to_string(),
                                options: opts,
                            },
                        );
                        viona_index += 1;
                        pci_index += 1;
                    }
                    EndpointKind::Sidemux(radix) => {
                        let mut opts = BTreeMap::new();
                        opts.insert("radix".to_string(), toml::Value::Integer(radix.try_into()?));
                        opts.insert(
                            "link-name".to_string(),
                            toml::Value::String(d.vnic_link_name(e)),
                        );
                        opts.insert(
                            "pci-path".to_string(),
                            toml::Value::String(format!("0.{}.0", pci_index)),
                        );
                        devices.insert(
                            format!("sidemux{}", sidemux_index),
                            propolis_server::config::Device {
                                driver: "sidemux".into(),
                                options: opts,
                            },
                        );
                        sidemux_index += 1;
                        pci_index += radix;
                    }
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
        fs::write(format!(".falcon/{}.toml", self.name), config_toml)?;

        Ok(())
    }

    async fn launch(&self, r: &Runner, port: u32) -> Result<(), Error> {
        // launch vm

        let id = uuid::Uuid::new_v4();
        launch_vm(&r.log, &r.propolis_binary, port, &id, &self).await?;

        // initial vm configuration

        let ws_sockaddr = format!("[::1]:{}", port);

        // login to serial console
        let mut sc = serial::SerialCommander::new(
            SocketAddr::from_str(ws_sockaddr.as_ref())?,
            id.to_string(),
            r.log.clone(),
        );
        let mut ws = sc.start().await?;

        // setup mounts
        // TODO this will only work as expected for one mount.
        for mount in &self.mounts {
            info!(r.log, "mouting {}", mount.destination);
            sc.exec(&mut ws, "p9kp load-driver".into()).await?;
            let cmd = format!(
                "mkdir -p {dst}; cd {dst}; p9kp pull",
                dst = mount.destination
            );
            sc.exec(&mut ws, cmd).await?;
            sc.exec(&mut ws, "cd".into()).await?;
        }

        // set hostname
        let cmd = format!("hostname {}", self.name);
        sc.exec(&mut ws, cmd).await?;
        let cmd = format!(
            "echo '::1 {name}.local {name}' >> /etc/hosts",
            name = self.name,
        );
        sc.exec(&mut ws, cmd).await?;
        let cmd = format!(
            "echo '127.0.0.1 {name}.local {name}' >> /etc/hosts",
            name = self.name,
        );
        sc.exec(&mut ws, cmd).await?;

        // log out and log back in to get updated console
        //let mut ws = sc.connect().await?;
        sc.logout(&mut ws).await?;
        sc.login(&mut ws).await?;

        Ok(())
    }

    fn node_name(&self, d: &Deployment) -> String {
        format!("{}_{}", d.name, self.name)
    }

    fn destroy(&self, r: &Runner) -> Result<(), Error> {
        // get propolis pid
        let pid = match fs::read_to_string(format!(".falcon/{}.pid", self.name)) {
            Ok(pid) => match i32::from_str_radix(pid.as_ref(), 10) {
                Ok(pid) => pid,
                Err(e) => {
                    warn!(r.log, "parse propolis pid for {}: {}", self.name, e);
                    return Ok(());
                }
            },
            Err(e) => {
                warn!(r.log, "get propolis pid for {}: {}", self.name, e);
                return Ok(());
            }
        };

        // kill propolis instance
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }

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
        match Command::new("bhyvectl")
            .args(&["--destroy", vm_arg.as_ref()])
            .output()
        {
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

            let slink_h = libnet::LinkHandle::Name(slink.clone());
            let vlink_h = libnet::LinkHandle::Name(vlink.clone());

            // if dangling links exists, remove them
            debug!(r.log, "destroying link {}", &vlink);
            libnet::delete_link(&vlink_h, libnet::LinkFlags::Active)?;
            debug!(r.log, "destroying link {}", &slink);
            libnet::delete_link(&slink_h, libnet::LinkFlags::Active)?;

            info!(r.log, "creating simnet link '{}'", &slink);
            libnet::create_simnet_link(&slink, libnet::LinkFlags::Active)?;

            info!(r.log, "creating vnic link '{}'", &vlink);
            libnet::create_vnic_link(&vlink, &slink_h, libnet::LinkFlags::Active)?;

            debug!(r.log, "link pair created");
        }

        // make point to point connection beteween interfaces
        let slink0 = d.simnet_link_name(&self.endpoints[0]);
        let slink1 = d.simnet_link_name(&self.endpoints[1]);
        let slink0_h = libnet::LinkHandle::Name(slink0);
        let slink1_h = libnet::LinkHandle::Name(slink1);
        libnet::connect_simnet_peers(&slink0_h, &slink1_h)?;

        Ok(())
    }

    fn destroy(&self, r: &Runner) -> Result<(), Error> {
        let d = &r.deployment;

        for e in self.endpoints.iter() {
            let slink = d.simnet_link_name(e);
            let vlink = d.vnic_link_name(e);
            let slink_h = libnet::LinkHandle::Name(slink.clone());
            let vlink_h = libnet::LinkHandle::Name(vlink.clone());

            info!(r.log, "destroying link {}", &vlink);
            libnet::delete_link(&vlink_h, libnet::LinkFlags::Active)?;
            info!(r.log, "destroying link {}", &slink);
            libnet::delete_link(&slink_h, libnet::LinkFlags::Active)?;
        }

        Ok(())
    }
}

impl ExtLink {
    fn create(&self, r: &Runner) -> Result<(), Error> {
        let vnic_name = r.deployment.vnic_link_name(&self.endpoint);
        let vnic = libnet::LinkHandle::Name(vnic_name.clone());
        let host_ifx = libnet::LinkHandle::Name(self.host_ifx.clone());

        // destroy any dangling links
        debug!(r.log, "destroying external link {}", &vnic_name);
        libnet::delete_link(&vnic, libnet::LinkFlags::Active)?;

        // create vnic
        info!(r.log, "creating external link {}", &vnic_name);
        libnet::create_vnic_link(&vnic_name, &host_ifx, libnet::LinkFlags::Active)?;

        debug!(
            r.log,
            "external link {}@{} created", &vnic_name, &self.host_ifx
        );

        Ok(())
    }

    fn destroy(&self, r: &Runner) -> Result<(), Error> {
        let vnic_name = r.deployment.vnic_link_name(&self.endpoint);
        let vnic = libnet::LinkHandle::Name(vnic_name.clone());
        info!(r.log, "destroying external link {}", &vnic_name);
        libnet::delete_link(&vnic, libnet::LinkFlags::Active)?;

        Ok(())
    }
}

pub(crate) async fn launch_vm(
    log: &Logger,
    propolis_binary: &String,
    port: u32,
    id: &uuid::Uuid,
    node: &Node,
) -> Result<(), Error> {
    // launch propolis-server

    fs::write(format!(".falcon/{}.port", node.name), port.to_string())?;

    let stdout = fs::File::create(format!(".falcon/{}.out", node.name))?;
    let stderr = fs::File::create(format!(".falcon/{}.err", node.name))?;
    let config = format!(".falcon/{}.toml", node.name);
    let sockaddr = format!("[::]:{}", port);
    let mut cmd = Command::new(propolis_binary);
    cmd.args(&["run", config.as_ref(), sockaddr.as_ref()])
        .stdout(stdout)
        .stderr(stderr);
    let child = cmd.spawn()?;

    fs::write(format!(".falcon/{}.pid", node.name), child.id().to_string())?;

    info!(
        log,
        "launched instance {} with pid {} on port {}",
        node.name,
        child.id(),
        port,
    );

    let sockaddr = format!("[::1]:{}", port);

    // create vm instance
    let client =
        propolis_client::Client::new(SocketAddr::from_str(sockaddr.as_ref())?, log.clone());

    fs::write(format!(".falcon/{}.uuid", node.name), id.to_string())?;

    let properties = propolis_client::api::InstanceProperties {
        id: *id,
        name: node.name.clone(),
        description: "a falcon vm".to_string(),
        image_id: uuid::Uuid::default(),
        bootrom_id: uuid::Uuid::default(),
        memory: node.memory,
        vcpus: node.cores,
    };
    let req = propolis_client::api::InstanceEnsureRequest {
        properties,
        nics: Vec::new(),
        disks: Vec::new(),
        migrate: None,
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
    client
        .instance_state_put(*id, propolis_client::api::InstanceStateRequested::Run)
        .await?;

    Ok(())
}

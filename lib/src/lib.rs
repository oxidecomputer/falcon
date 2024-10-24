// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

#[cfg(test)]
mod test;
mod util;

pub mod cli;
pub mod error;
pub mod serial;
pub mod unit;

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use error::Error;
use futures::future::join_all;
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use propolis_client::types::InstanceMetadata;
use propolis_server_config::{BlockDevice, BlockOpts, Device};
use ron::ser::{to_string_pretty, PrettyConfig};
use serde::{Deserialize, Serialize};
use slog::Drain;
use slog::{debug, error, info, warn, Logger};
use std::collections::BTreeMap;
use std::convert::TryInto;
use std::fs::{self, OpenOptions};
use std::io::{Read, BufWriter};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::{sleep, Duration, Instant};
use xz2::read::XzDecoder;

#[macro_export]
macro_rules! node {
    ($d:ident, $name:ident, $img:literal, $cores:literal, $mem:expr) => {
        let $name = $d.node(stringify!($name), $img, $cores, $mem);
    };

    ($d:ident, $name:ident, $img:ident, $cores:literal, $mem:expr) => {
        let $name = $d.node(stringify!($name), $img, $cores, $mem);
    };
}

#[macro_export]
macro_rules! cmd {
    ($d:expr, $node:expr, $cmd:expr, $msg:expr) => {{
        let bx: std::pin::Pin<
            Box<
                dyn futures::future::Future<
                    Output = Result<String, anyhow::Error>,
                >,
            >,
        > = Box::pin(async {
            info!($d.log, "{} start", $msg);
            let result = $d
                .exec($node, $cmd)
                .await
                .map_err(|e| anyhow!("{} {e}", $msg));
            info!($d.log, "{} finish", $msg);
            result
        });
        bx
    }};
}

pub const DEFAULT_FALCON_DIR: &str = ".falcon";
const ZFS_BIN: &str = "/usr/sbin/zfs";
const DLADM_BIN: &str = "/usr/sbin/dladm";
const DD_BIN: &str = "/usr/bin/dd";
const RM_BIN: &str = "/usr/bin/rm";
const TRUNCATE_BIN: &str = "/usr/bin/truncate";

pub struct Runner {
    /// The deployment object that describes the Falcon topology
    pub deployment: Deployment,

    /// If persistent is set to true, this deployment will not autodestruct when
    /// dropped.
    pub persistent: bool,

    /// The propolis-server binary to use
    pub propolis_binary: String,

    pub log: Logger,

    /// The root dataset to use for falcon activities
    pub dataset: String,

    /// The location of the ".falcon" directory for a given deployment
    ///
    /// This directory is created by falcon and stores configuration.
    pub falcon_dir: Utf8PathBuf,
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

    /// External links connected to a host data link such as a phy or a vnic.
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

#[derive(Serialize, Deserialize)]
pub enum PrimaryDiskBacking {
    /// Use a zvol cloned from the image source.
    Zvol,
    /// Use a file copied from the image source.
    File,
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
    /// The root dataset to use for falcon activities
    pub dataset: String,
    /// Whether or not to do initial setup on the node
    pub do_setup: bool,
    /// How much space to reserve on the boot disk in GB.
    pub reserved: usize,
    /// How to create the backing of the main disk.
    pub primary_disk_backing: PrimaryDiskBacking,
    /// VNC port to use
    pub vnc_port: Option<u16>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum GuestMountMechanism {
    P9kp,
    Mount,
}

/// Directories mounted from host machine into a node.
#[derive(Debug, Serialize, Deserialize)]
pub struct Mount {
    /// Directory from host to mount.
    pub source: Utf8PathBuf,

    /// Directory in node to mount to.
    pub destination: Utf8PathBuf,

    /// Mechanism to mount in the guest.
    pub mechanism: GuestMountMechanism,
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
    /// Use a bhyve/viona kernel device. This is the default. Optionally specify
    /// mac.
    Viona(Option<String>),

    /// Use a Sidecar multiplexing device. If you are unsure, this is not what
    /// you want. The usize parameter indicates radix of the connected Sidecar
    /// device. May optionally specify macs for sidemux ports.
    Sidemux(usize, Option<Vec<String>>),

    /// A link connected to a SoftNPU device with an optional MAC specification
    SoftNPU(Option<String>),
}

impl EndpointKind {
    fn designator(&self) -> &'static str {
        match self {
            Self::Viona(_) => "vn",
            Self::Sidemux(_, _) => "sm",
            Self::SoftNPU(_) => "sn",
        }
    }
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
                    unsafe {
                        std::env::set_var("RUST_LOG", "info");
                    }
                }
            }
            _ => unsafe {
                std::env::set_var("RUST_LOG", "info");
            },
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
            dataset: dataset(),
            falcon_dir: DEFAULT_FALCON_DIR.into(),
        }
    }

    /// Create a new node within this deployment with the given name. Names must
    /// conform to `[A-Za-z]?[A-Za-z0-9_]*`
    pub fn node(
        &mut self,
        name: &str,
        image: &str,
        cores: u8,
        memory: u64,
    ) -> NodeRef {
        namecheck!(name, "node");

        let id = uuid::Uuid::new_v4();

        let r = NodeRef {
            index: self.deployment.nodes.len(),
        };
        let n = Node {
            name: String::from(name),
            image: String::from(image),
            dataset: self.dataset.clone(),
            radix: 0,
            mounts: Vec::new(),
            id,
            cores,
            memory,
            do_setup: true,
            reserved: 20,
            primary_disk_backing: PrimaryDiskBacking::Zvol,
            vnc_port: None,
        };
        self.deployment.nodes.push(n);
        r
    }

    pub fn find_node(&self, name: &str) -> Option<NodeRef> {
        Some(NodeRef {
            index: self.deployment.nodes.iter().position(|x| x.name == name)?,
        })
    }

    pub fn all_nodes(&self) -> Vec<NodeRef> {
        let mut result = Vec::new();
        for index in 0..self.deployment.nodes.len() {
            result.push(NodeRef { index })
        }
        result
    }

    pub fn get_node(&self, r: NodeRef) -> &Node {
        &self.deployment.nodes[r.index]
    }

    pub fn do_setup(&mut self, r: NodeRef, value: bool) {
        self.deployment.nodes[r.index].do_setup = value;
    }

    pub fn bump_radix(&mut self, node: NodeRef) -> usize {
        let current = self.deployment.nodes[node.index].radix;
        self.deployment.nodes[node.index].radix += 1;
        current
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
                    kind: EndpointKind::Viona(None),
                },
                Endpoint {
                    node: b,
                    index: self.deployment.nodes[b.index].radix,
                    kind: EndpointKind::Viona(None),
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
    pub fn sidecar_link(
        &mut self,
        sidecar: NodeRef,
        controller: NodeRef,
        radix: usize,
        macs: Option<Vec<String>>,
    ) -> LinkRef {
        let r = LinkRef {
            _index: self.deployment.links.len(),
        };
        let l = Link {
            endpoints: [
                Endpoint {
                    node: sidecar,
                    index: self.bump_radix(sidecar),
                    kind: EndpointKind::Viona(None),
                },
                Endpoint {
                    node: controller,
                    index: self.bump_radix(controller),
                    kind: EndpointKind::Sidemux(radix, macs),
                },
            ],
        };
        self.deployment.links.push(l);
        r
    }

    pub fn softnpu_link(
        &mut self,
        softnpu_node: NodeRef,
        node: NodeRef,
        node_mac: Option<String>,
        softnpu_mac: Option<String>,
    ) -> LinkRef {
        let r = LinkRef {
            _index: self.deployment.links.len(),
        };
        let l = Link {
            endpoints: [
                Endpoint {
                    node: softnpu_node,
                    index: self.deployment.nodes[softnpu_node.index].radix,
                    kind: EndpointKind::SoftNPU(softnpu_mac),
                },
                Endpoint {
                    node,
                    index: self.deployment.nodes[node.index].radix,
                    kind: EndpointKind::Viona(node_mac),
                },
            ],
        };
        self.deployment.links.push(l);
        self.deployment.nodes[softnpu_node.index].radix += 1;
        self.deployment.nodes[node.index].radix += 1;
        r
    }

    pub fn softnpu_links(
        &mut self,
        node1: NodeRef,
        node2: NodeRef,
        mac1: Option<String>,
        mac2: Option<String>,
    ) -> LinkRef {
        let r = LinkRef {
            _index: self.deployment.links.len(),
        };
        let l = Link {
            endpoints: [
                Endpoint {
                    node: node1,
                    index: self.deployment.nodes[node1.index].radix,
                    kind: EndpointKind::SoftNPU(mac1),
                },
                Endpoint {
                    node: node2,
                    index: self.deployment.nodes[node2.index].radix,
                    kind: EndpointKind::SoftNPU(mac2),
                },
            ],
        };
        self.deployment.links.push(l);
        self.deployment.nodes[node1.index].radix += 1;
        self.deployment.nodes[node2.index].radix += 1;
        r
    }

    pub fn reserve(&mut self, n: NodeRef, gb: usize) {
        self.deployment.nodes[n.index].reserved = gb;
    }

    pub fn set_backing(&mut self, n: NodeRef, backing: PrimaryDiskBacking) {
        self.deployment.nodes[n.index].primary_disk_backing = backing
    }

    /// Create an external link attached to `host_ifx`.
    pub fn ext_link(&mut self, host_ifx: impl AsRef<str>, n: NodeRef) {
        let endpoint = Endpoint {
            node: n,
            index: self.deployment.nodes[n.index].radix,
            kind: EndpointKind::Viona(None),
        };
        let host_ifx = host_ifx.as_ref().into();
        self.deployment
            .ext_links
            .push(ExtLink { endpoint, host_ifx });
        self.deployment.nodes[n.index].radix += 1;
    }

    /// Provide the host folder `src` as a p9fs mount to the guest with the tag
    /// `dst`.
    pub fn do_mount(
        &mut self,
        src: impl AsRef<Utf8Path>,
        dst: impl AsRef<Utf8Path>,
        n: NodeRef,
        mechanism: GuestMountMechanism,
    ) -> Result<(), Error> {
        let src = src.as_ref();
        let src = src.canonicalize_utf8().map_err(|error| {
            Error::PathError(format!(
                "{}: canonicalization error: {}",
                src, error
            ))
        })?;

        self.deployment.nodes[n.index].mounts.push(Mount {
            source: src,
            destination: dst.as_ref().to_owned(),
            mechanism,
        });

        Ok(())
    }

    pub fn mount(
        &mut self,
        src: impl AsRef<Utf8Path>,
        dst: impl AsRef<Utf8Path>,
        n: NodeRef,
    ) -> Result<(), Error> {
        self.do_mount(src, dst, n, GuestMountMechanism::P9kp)
    }

    pub fn mount_linux(
        &mut self,
        src: impl AsRef<Utf8Path>,
        dst: impl AsRef<Utf8Path>,
        n: NodeRef,
    ) -> Result<(), Error> {
        self.do_mount(src, dst, n, GuestMountMechanism::Mount)
    }

    /// Launch the deployment. This will clone the necessary image zvols, create
    /// the propolis VM instances, create the point to point network interfaces,
    /// set up the serial console for each VM and, run any user defined exec
    /// statements.
    pub async fn launch(&self) -> Result<(), Error> {
        self.preflight().await?;
        match self.do_launch().await {
            Ok(()) => Ok(()),
            Err(e) => {
                error!(self.log, "launch failed: {}", e);
                Err(e)
            }
        }
    }

    async fn preflight(&self) -> Result<(), Error> {
        // Verify all required executables are discoverable.
        let out = Command::new(&self.propolis_binary).args(["-V"]).output();
        if out.is_err() {
            return Err(Error::Exec(format!(
                "failed to find {} on PATH",
                &self.propolis_binary
            )));
        }

        // ensure falcon working dir
        fs::create_dir_all(&self.falcon_dir)?;

        // write falcon config
        let pretty = PrettyConfig::new().separate_tuple_members(true);
        let out = format!("{}\n", to_string_pretty(&self.deployment, pretty)?);
        let mut topo_path = self.falcon_dir.clone();
        topo_path.push("topology.ron");
        fs::write(&topo_path, out)?;

        for n in self.deployment.nodes.iter() {
            n.preflight(self).await?;
        }

        Ok(())
    }

    async fn net_launch(&self) -> Result<(), Error> {
        info!(self.log, "creating links");
        for l in self.deployment.links.iter() {
            l.create(self)?;
        }

        info!(self.log, "creating external links");
        for l in self.deployment.ext_links.iter() {
            l.create(self)?;
        }

        Ok(())
    }

    async fn do_launch(&self) -> Result<(), Error> {
        self.net_launch().await?;

        info!(self.log, "creating nodes");

        let mut fs = Vec::new();
        for n in self.deployment.nodes.iter() {
            fs.push(n.launch(self));
        }
        for x in join_all(fs).await {
            x?;
        }

        Ok(())
    }

    pub fn net_destroy(&self) -> Result<(), Error> {
        info!(self.log, "destroying links");
        for l in self.deployment.links.iter() {
            l.destroy(self)?;
        }

        info!(self.log, "destroying external links");
        for l in self.deployment.ext_links.iter() {
            l.destroy(self)?;
        }
        Ok(())
    }

    /// Tear down all the nodes, followed by the links and the ZFS pool
    // TODO in parallel
    pub fn destroy(&self) -> Result<(), Error> {
        info!(self.log, "destroying nodes");
        for n in self.deployment.nodes.iter() {
            n.destroy(self)?;
        }

        self.net_destroy()?;

        // Destroy images
        info!(self.log, "destroying images");

        // destroy any zvol backed images
        let img_dir = format!("{}/topo/{}", self.dataset, self.deployment.name);
        Command::new(ZFS_BIN)
            .args(["destroy", "-r", img_dir.as_ref()])
            .output()?;

        // destroy any file backed images
        let img_dir = format!("/var/falcon/dsk/{}", self.deployment.name);
        Command::new(RM_BIN)
            .args(["-rf", img_dir.as_ref()])
            .output()?;

        // Destroy workspace
        info!(self.log, "destroying workspace");
        fs::remove_dir_all(&self.falcon_dir)?;

        Ok(())
    }

    /// Run a command synchronously in the vm.
    pub async fn exec(&self, n: NodeRef, cmd: &str) -> Result<String, Error> {
        let name = self.deployment.nodes[n.index].name.clone();
        self.do_exec(&name, cmd).await
    }

    async fn do_exec(&self, name: &str, cmd: &str) -> Result<String, Error> {
        let mut path = self.falcon_dir.clone();
        path.push(format!("{name}.uuid"));
        let id = match fs::read_to_string(&path) {
            Ok(u) => u,
            Err(e) => {
                return Err(Error::NotFound(format!(
                    "propolis uuid for {}: {}",
                    name, e
                )));
            }
        };
        path.pop();

        path.push(format!("{name}.port"));
        let port = match fs::read_to_string(&path) {
            Ok(p) => p.as_str().parse::<u16>()?,
            Err(e) => {
                return Err(Error::NotFound(format!(
                    "get propolis port for {}: {}",
                    name, e
                )));
            }
        };
        path.pop();

        let addr = SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
            port,
        );

        let mut sc = serial::SerialCommander::new(
            addr,
            id,
            name.into(),
            self.log.clone(),
        );
        let mut ws = sc.start(true).await?;
        let out = sc.exec(&mut ws, cmd.to_string()).await?;
        sc.logout(&mut ws).await?;
        Ok(out)
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
            "{}_{}_{}_sim{}",
            self.name,
            self.nodes[e.node.index].name,
            e.kind.designator(),
            e.index,
        )
    }

    fn vnic_link_name(&self, e: &Endpoint) -> String {
        format!(
            "{}_{}_{}_vnic{}",
            self.name,
            self.nodes[e.node.index].name,
            e.kind.designator(),
            e.index,
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
    async fn preflight(&self, r: &Runner) -> Result<(), Error> {
        let mut devices = BTreeMap::new();
        let mut block_devs = BTreeMap::new();

        self.try_ensure_base_image(&r.log).await?;

        let backing = match self.primary_disk_backing {
            PrimaryDiskBacking::Zvol => self.create_zvol_backing(r)?,
            PrimaryDiskBacking::File => self.create_file_backing(r)?,
        };
        self.create_blockdev(backing, &mut devices, &mut block_devs);

        let mut pci_index = 5;

        // mounts
        for (i, m) in self.mounts.iter().enumerate() {
            let mut opts = BTreeMap::new();
            opts.insert("source".to_string(), m.source.to_string().into());
            opts.insert("target".to_string(), m.destination.to_string().into());
            opts.insert(
                "pci-path".to_string(),
                toml::Value::String(format!("0.{}.0", pci_index)),
            );

            devices.insert(
                format!("fs{}", i),
                propolis_server_config::Device {
                    driver: "pci-virtio-9p".to_string(),
                    options: opts,
                },
            );
            pci_index += 1;
        }

        // network interfaces
        let d = &r.deployment;

        //let mut links: Vec<String> = Vec::new();
        let mut viona_index = 0;
        let mut softnpu_index = 0;
        let mut sidemux_index = 0;

        let mut endpoints = Vec::new();
        for l in &d.links {
            endpoints.extend_from_slice(&l.endpoints);
        }
        for l in &d.ext_links {
            endpoints.push(l.endpoint.clone());
        }

        let has_softnpu = endpoints
            .iter()
            .any(|x| matches!(&x.kind, EndpointKind::SoftNPU(_)));

        if has_softnpu {
            let mut opts = BTreeMap::new();
            opts.insert(
                "pci-path".to_string(),
                toml::Value::String(format!("0.{}.0", pci_index)),
            );

            devices.insert(
                "softnpup9".to_owned(),
                propolis_server_config::Device {
                    driver: "softnpu-p9".to_string(),
                    options: opts,
                },
            );
            pci_index += 1;

            let mut opts = BTreeMap::new();
            opts.insert(
                "pci-path".to_string(),
                toml::Value::String(format!("0.{}.0", pci_index)),
            );

            devices.insert(
                "softnpu-pci-port".to_owned(),
                propolis_server_config::Device {
                    driver: "softnpu-pci-port".to_string(),
                    options: opts,
                },
            );
            pci_index += 1;
        }

        for e in &endpoints {
            if d.nodes[e.node.index].name == self.name {
                match &e.kind {
                    EndpointKind::Viona(_) => {
                        //links.push(d.vnic_link_name(e));
                        let mut opts = BTreeMap::new();
                        opts.insert(
                            "vnic".to_string(),
                            toml::Value::String(d.vnic_link_name(e)),
                        );
                        opts.insert(
                            "pci-path".to_string(),
                            toml::Value::String(format!("0.{}.0", pci_index)),
                        );
                        devices.insert(
                            format!("net{}", viona_index),
                            propolis_server_config::Device {
                                driver: "pci-virtio-viona".to_string(),
                                options: opts,
                            },
                        );
                        viona_index += 1;
                        pci_index += 1;
                    }
                    EndpointKind::Sidemux(radix, macs) => {
                        let mut opts = BTreeMap::new();
                        opts.insert(
                            "radix".to_string(),
                            toml::Value::Integer((*radix).try_into()?),
                        );
                        opts.insert(
                            "link-name".to_string(),
                            toml::Value::String(d.vnic_link_name(e)),
                        );
                        opts.insert(
                            "pci-path".to_string(),
                            toml::Value::String(format!("0.{}.0", pci_index)),
                        );
                        match macs {
                            Some(macs) => {
                                opts.insert(
                                    "macs".to_string(),
                                    toml::Value::Array(
                                        macs.iter()
                                            .map(|x| {
                                                toml::Value::String(x.clone())
                                            })
                                            .collect(),
                                    ),
                                );
                            }
                            None => {}
                        }
                        devices.insert(
                            format!("sidemux{}", sidemux_index),
                            propolis_server_config::Device {
                                driver: "sidemux".into(),
                                options: opts,
                            },
                        );
                        sidemux_index += 1;
                        // +1 on the radix is for the pci port
                        pci_index += radix + 1;
                    }
                    EndpointKind::SoftNPU(mac) => {
                        let mut opts = BTreeMap::new();
                        opts.insert(
                            "vnic".to_string(),
                            toml::Value::String(d.vnic_link_name(e)),
                        );
                        match mac {
                            Some(ref mac) => {
                                opts.insert(
                                    "mac".to_string(),
                                    toml::Value::String(mac.clone()),
                                );
                            }
                            None => {}
                        };
                        devices.insert(
                            format!("port{}", softnpu_index),
                            propolis_server_config::Device {
                                driver: "softnpu-port".to_string(),
                                options: opts,
                            },
                        );
                        softnpu_index += 1;
                    }
                }
            }
        }

        let chipset = propolis_server_config::Chipset {
            options: BTreeMap::new(),
        };

        // write propolis instance config to <falcon_dir>/<node-name>.toml

        let propolis_config = propolis_server_config::Config {
            bootrom: PathBuf::from("/var/ovmf/OVMF_CODE.fd"),
            chipset,
            devices,
            block_devs,
            ..Default::default()
        };

        let config_toml = toml::to_string(&propolis_config)?;

        let mut path = r.falcon_dir.clone();
        path.push(format!("{}.toml", self.name));
        fs::write(&path, config_toml)?;

        Ok(())
    }

    async fn try_ensure_base_image(&self, log: &Logger) -> Result<(), Error> {
        match Command::new(ZFS_BIN)
            .args([
                "list",
                "-t",
                "snapshot",
                format!("{}/img/{}@base", self.dataset, self.image).as_str(),
            ])
            .output()
        {
            Ok(output) if output.status.success() => Ok(()),
            _ => {
                info!(
                    log,
                    "base image for {} does not exist, attempting to install",
                    self.image
                );
                self.try_install_base_image(log).await
            }
        }
    }

    async fn try_install_base_image(&self, log: &Logger) -> Result<(), Error> {
        let iname = format!("{}_0.raw.xz", self.image);
        let path = format!("/tmp/{iname}");
        self.try_download_base_image(log, iname.as_str(), path.as_str())
            .await?;
        self.try_extract_image_to_new_volume(log, path.as_str())?;
        Ok(())
    }

    fn try_extract_image_to_new_volume(
        &self,
        log: &Logger,
        from: &str,
    ) -> Result<(), Error> {
        info!(log, "calculating image size");
        let pb = Self::new_progress_bar();
        let in_file = std::fs::File::open(from)?;
        let len = in_file
            .metadata()
            .context("compressed image metadata")?
            .len();
        pb.inc_length(len);
        let in_file = pb.wrap_read(in_file);
        let dec = XzDecoder::new(in_file);
        let fsize = dec.bytes().count();
        pb.finish();

        let zpath = format!("{}/img/{}", self.dataset, self.image);
        let bsize = fsize + 4096 - fsize % 4096;
        info!(log, "creating zvol {zpath} of size {bsize}");
        let out = Command::new(ZFS_BIN)
            .args([
                "create",
                "-p",
                "-V",
                &bsize.to_string(),
                "-o",
                "volblocksize=4k",
                zpath.as_str(),
            ])
            .output()
            .context("zfs create volume")?;

        if !out.status.success() {
            return Err(Error::Exec(format!(
                "zfs create vol: {} {}",
                String::from_utf8_lossy(out.stdout.as_slice()),
                String::from_utf8_lossy(out.stderr.as_slice())
            )));
        }

        info!(log, "extracting image to zvol");
        let in_file = std::fs::File::open(from)?;
        let dec = XzDecoder::new(in_file);

        let dst = OpenOptions::new().write(true).open(format!(
            "/dev/zvol/rdsk/{}/img/{}",
            self.dataset, self.image
        ))?;
        let pb = Self::new_progress_bar();
        pb.inc_length(dst.metadata().context("zvol dst metadata")?.len());
        let mut dst = BufWriter::with_capacity(1024 * 1024, dst);

        std::io::copy(&mut pb.wrap_read(dec), &mut dst)
            .context("copy image to zfs vol")?;

        pb.finish();

        let spath = format!("{}/img/{}@base", self.dataset, self.image);
        info!(log, "creating zfs snapshot {spath}");
        let out = Command::new(ZFS_BIN)
            .args(["snapshot", spath.as_str()])
            .output()
            .context("zfs create snapshot")?;
        if !out.status.success() {
            return Err(Error::Exec(format!(
                "zfs create snapshot: {} {}",
                String::from_utf8_lossy(out.stdout.as_slice()),
                String::from_utf8_lossy(out.stderr.as_slice())
            )));
        }
        Ok(())
    }

    fn new_progress_bar() -> ProgressBar {
        let pb = ProgressBar::new(0);
        let sty = ProgressStyle::with_template(
            "[{elapsed_precise}] \
            {bar:40.cyan/blue} \
            {bytes}/{total_bytes} \
            {msg}",
        )
        .unwrap()
        .progress_chars("##-");
        pb.set_style(sty);
        pb
    }

    async fn try_download_base_image(
        &self,
        log: &Logger,
        iname: &str,
        path: &str,
    ) -> Result<(), Error> {
        if Path::new(path).exists() {
            info!(log, "image already downloaded");
            return Ok(());
        }
        let url = format!(
            "https://oxide-falcon-assets.s3.us-west-2.amazonaws.com/{iname}"
        );
        info!(log, "trying to download {url}");

        let pb = Self::new_progress_bar();

        let client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(3600))
            .tcp_keepalive(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .unwrap();
        let response = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to get url {url}"))?;

        if !response.status().is_success() {
            Err(anyhow::anyhow!(
                "failed to download image: {}",
                response.status()
            ))?;
        }
        pb.inc_length(
            response
                .content_length()
                .ok_or_else(|| anyhow::anyhow!("Missing content length"))?,
        );
        let mut file = tokio::fs::File::create(path)
            .await
            .with_context(|| format!("failed to create {path}"))?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.with_context(|| {
                format!("failed reading response from {url}")
            })?;
            file.write_all(&chunk)
                .await
                .with_context(|| format!("failed writing {path:?}"))?;
            pb.inc(chunk.len().try_into().unwrap());
        }
        pb.finish();
        Ok(())
    }

    fn create_zvol_backing(&self, r: &Runner) -> Result<String, Error> {
        //Clone base image

        //TODO incorporate version into img
        let source = format!("{}/img/{}@base", self.dataset, self.image);
        let dest = format!(
            "{}/topo/{}/{}",
            self.dataset, r.deployment.name, self.name
        );

        let out = Command::new(ZFS_BIN)
            .args(["clone", "-p", source.as_ref(), dest.as_ref()])
            .output()?;

        if !out.status.success() {
            return Err(Error::Zfs(String::from_utf8(out.stderr)?));
        }

        let volsize = format!("volsize={}G", self.reserved);

        let out = Command::new(ZFS_BIN)
            .args(["set", volsize.as_str(), dest.as_ref()])
            .output()?;

        if !out.status.success() {
            return Err(Error::Zfs(String::from_utf8(out.stderr)?));
        }

        let reserved = format!("reservation={}G", self.reserved);

        let out = Command::new(ZFS_BIN)
            .args(["set", reserved.as_str(), dest.as_ref()])
            .output()?;

        if !out.status.success() {
            return Err(Error::Zfs(String::from_utf8(out.stderr)?));
        }

        let out = Command::new(ZFS_BIN)
            .args(["set", "sync=disabled", dest.as_ref()])
            .output()?;

        if !out.status.success() {
            return Err(Error::Zfs(String::from_utf8(out.stderr)?));
        }

        let zvol = format!(
            "/dev/zvol/rdsk/{}/topo/{}/{}",
            self.dataset, r.deployment.name, self.name,
        );

        Ok(zvol)
    }

    fn create_file_backing(&self, r: &Runner) -> Result<String, Error> {
        let size = format!("{}G", self.reserved);

        let dir = format!("/var/falcon/dsk/{}", r.deployment.name);
        if let Err(e) = fs::create_dir_all(&dir) {
            error!(r.log, "failed to create image directory: {e}");
            return Err(Error::IO(e));
        }
        let backing = format!("{}/{}", dir, self.name);
        let source_zvol =
            format!("/dev/zvol/dsk/{}/img/{}@base", self.dataset, self.image);

        info!(r.log, "copying backing image for {}", self.name);
        let dd_if = format!("if={source_zvol}");
        let dd_of = format!("of={backing}");
        let out = Command::new(DD_BIN)
            .args([dd_if.as_str(), dd_of.as_str(), "bs=1024M"])
            .output()?;
        if !out.status.success() {
            return Err(Error::Exec(String::from_utf8(out.stderr)?));
        }

        let out = Command::new(TRUNCATE_BIN)
            .args(["-s", size.as_str(), backing.as_str()])
            .output()?;
        if !out.status.success() {
            return Err(Error::Exec(String::from_utf8(out.stderr)?));
        }

        Ok(backing)
    }

    fn create_blockdev(
        &self,
        backing: String,
        devices: &mut BTreeMap<String, Device>,
        block_devs: &mut BTreeMap<String, BlockDevice>,
    ) {
        let mut device_options = BTreeMap::new();
        let mut blockdev_options = BTreeMap::new();
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
            propolis_server_config::Device {
                driver: "pci-virtio-block".to_string(),
                options: device_options,
            },
        );
        blockdev_options
            .insert("path".to_string(), toml::Value::String(backing));
        block_devs.insert(
            "main_disk".to_string(),
            propolis_server_config::BlockDevice {
                bdtype: "file".to_string(),
                options: blockdev_options,
                opts: BlockOpts {
                    block_size: None,
                    read_only: None,
                    skip_flush: Some(true),
                },
            },
        );
    }

    async fn launch(&self, r: &Runner) -> Result<(), Error> {
        // launch vm

        let id = uuid::Uuid::new_v4();
        let port =
            launch_vm(&r.log, &r.propolis_binary, &id, self, &r.falcon_dir)
                .await?;

        if !self.do_setup {
            return Ok(());
        }

        // initial vm configuration

        let ws_sockaddr = format!("[::1]:{}", port);

        // login to serial console
        let mut sc = serial::SerialCommander::new(
            SocketAddr::from_str(ws_sockaddr.as_ref())?,
            id.to_string(),
            self.name.clone(),
            r.log.clone(),
        );
        let mut ws = sc.start(false).await?;

        // setup mounts
        // TODO this will only work as expected for one mount.
        for mount in &self.mounts {
            info!(r.log, "{}: mounting {}", self.name, mount.destination);
            let cmd = if mount.mechanism == GuestMountMechanism::Mount {
                format!(
                    "mkdir -p {dst}; mount -t 9p -o ro,msize=65536 {dst} {dst}",
                    dst = mount.destination
                )
            } else {
                format!(
                    "mkdir -p {dst}; cd {dst}; p9kp pull",
                    dst = mount.destination
                )
            };
            sc.exec(&mut ws, cmd).await?;
            sc.exec(&mut ws, "cd".into()).await?;
            info!(
                r.log,
                "{}: finished mounting {}", self.name, mount.destination
            );
        }

        // set hostname
        let cmd = format!("hostname {}", self.name);
        sc.exec(&mut ws, cmd).await?;
        let cmd = format!("echo '{name}' > /etc/nodename", name = self.name,);
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

        // log out after finishing setup
        info!(r.log, "{}: logging out", self.name);
        sc.logout(&mut ws).await?;
        info!(r.log, "{}: logged out", self.name);

        Ok(())
    }

    fn destroy(&self, r: &Runner) -> Result<(), Error> {
        // get propolis pid
        let mut path = r.falcon_dir.clone();
        path.push(format!("{}.pid", self.name));
        let pid = match fs::read_to_string(&path) {
            Ok(pid) => match pid.parse::<i32>() {
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
        path.pop();

        // kill propolis instance
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }

        // get instance uuid
        path.push(format!("{}.uuid", self.name));
        let uuid = match fs::read_to_string(&path) {
            Ok(u) => u,
            Err(e) => {
                warn!(r.log, "get propolis uuid for {}: {}", self.name, e);
                return Ok(());
            }
        };

        // destroy bhyve vm
        let vm_arg = format!("--vm={}", uuid);
        match Command::new("bhyvectl")
            .args(["--destroy", vm_arg.as_ref()])
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
            libnet_retry(|| {
                libnet::delete_link(&vlink_h, libnet::LinkFlags::Active)
            })?;
            debug!(r.log, "destroying link {}", &slink);
            libnet_retry(|| {
                libnet::delete_link(&slink_h, libnet::LinkFlags::Active)
            })?;

            info!(r.log, "creating simnet link '{}'", &slink);
            libnet::create_simnet_link(&slink, libnet::LinkFlags::Active)?;

            info!(r.log, "creating vnic link '{}'", &vlink);

            let mac = if let EndpointKind::Viona(Some(mac)) = &e.kind {
                let parts = mac.split(':');
                let mut v = Vec::new();
                for p in parts {
                    let x = u8::from_str_radix(p, 16)?;
                    v.push(x);
                }
                Some(v)
            } else {
                None
            };

            libnet::create_vnic_link(
                &vlink,
                &slink_h,
                mac,
                libnet::LinkFlags::Active,
            )?;
            let args =
                vec!["set-linkprop", "-p", "promisc-filtered=off", &vlink];
            match Command::new(DLADM_BIN).args(args).output() {
                Err(e) => {
                    return Err(Error::Exec(format!(
                        "failed to run {DLADM_BIN}: {e:?}"
                    )));
                }
                Ok(s) => {
                    if !s.status.success() {
                        return Err(Error::Exec(format!(
                            "{DLADM_BIN} failed: {:?}",
                            s.stderr
                        )));
                    }
                }
            }

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
            libnet_retry(|| {
                libnet::delete_link(&vlink_h, libnet::LinkFlags::Active)
            })?;
            info!(r.log, "destroying link {}", &slink);
            libnet_retry(|| {
                libnet::delete_link(&slink_h, libnet::LinkFlags::Active)
            })?;
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
        libnet_retry(|| libnet::delete_link(&vnic, libnet::LinkFlags::Active))?;

        // create vnic
        info!(r.log, "creating external link {}", &vnic_name);
        libnet::create_vnic_link(
            &vnic_name,
            &host_ifx,
            None,
            libnet::LinkFlags::Active,
        )?;

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
        libnet_retry(|| libnet::delete_link(&vnic, libnet::LinkFlags::Active))?;

        Ok(())
    }
}

pub(crate) async fn launch_vm(
    log: &Logger,
    propolis_binary: &str,
    id: &uuid::Uuid,
    node: &Node,
    falcon_dir: &Utf8Path,
) -> Result<u16, Error> {
    // launch propolis-server

    let mut path = falcon_dir.to_path_buf();
    path.push(format!("{}.out", node.name));
    let stdout = fs::File::create(&path)?;
    path.pop();
    path.push(format!("{}.err", node.name));
    let stderr = fs::File::create(&path)?;
    path.pop();
    path.push(format!("{}.toml", node.name));
    let config = path.clone();
    let sockaddr = String::from("[::]:0");
    let mut cmd = Command::new(propolis_binary);
    let mut args =
        vec!["run".to_string(), config.into_string(), sockaddr.clone()];
    if let Some(vnc_port) = node.vnc_port {
        args.push(format!("[::]:{}", vnc_port));
    }
    cmd.args(&args).stdout(stdout).stderr(stderr);
    let child = cmd.spawn()?;
    path.pop();

    path.push(format!("{}.pid", node.name));
    fs::write(&path, child.id().to_string())?;
    path.pop();

    let port = find_propolis_port_in_log(format!(".falcon/{}.out", node.name))
        .await
        .map_err(|e| anyhow::anyhow!("find propolis port in log: {e}"))?;

    path.push(format!("{}.port", node.name));
    fs::write(&path, port.to_string())?;
    path.pop();

    info!(
        log,
        "launched instance {} with pid {} on port {}",
        node.name,
        child.id(),
        port,
    );

    let sockaddr = format!("[::1]:{}", port);

    // create vm instance
    // We use a custom client builder here because the default progenitor
    // one has a timeout of 15s but we want to be able to wait indefinitely.
    let reqwest_client = reqwest::ClientBuilder::new().build().unwrap();
    let client = propolis_client::Client::new_with_client(
        &format!("http://{}", sockaddr),
        reqwest_client,
    );

    // https://github.com/rust-lang/rust-clippy/issues/9317
    #[allow(clippy::unnecessary_to_owned)]
    path.push(format!("{}.uuid", node.name));
    fs::write(&path, id.to_string())?;
    path.pop();

    let properties = propolis_client::types::InstanceProperties {
        id: *id,
        name: node.name.clone(),
        description: "a falcon vm".to_string(),
        image_id: uuid::Uuid::default(),
        bootrom_id: uuid::Uuid::default(),
        memory: node.memory,
        vcpus: node.cores,
        metadata: InstanceMetadata {
            project_id: uuid::Uuid::nil(),
            silo_id: uuid::Uuid::nil(),
            sled_id: uuid::Uuid::nil(),
            sled_model: "falcon".to_owned(),
            sled_serial: "falcon".to_owned(),
            sled_revision: 0,
        },
    };
    let req = propolis_client::types::InstanceEnsureRequest {
        properties,
        nics: Vec::new(),
        disks: Vec::new(),
        migrate: None,
        cloud_init_bytes: None,
    };

    // we just launched the instance, so wait for it to become ready
    let mut success = false;
    for _ in 0..30 {
        info!(log, "instance ensure: {}", node.name);
        match client.instance_ensure().body(&req).send().await {
            Ok(_) => {
                success = true;
                break;
            }
            Err(e) => {
                debug!(log, "instance ensure error: {e}, retry in 1 second");
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        }
    }
    if !success {
        client.instance_ensure().body(&req).send().await?;
    }

    info!(log, "instance run: {}", node.name);
    // run vm instance
    client
        .instance_state_put()
        .body(propolis_client::types::InstanceStateRequested::Run)
        .send()
        .await?;

    Ok(port)
}

pub(crate) fn dataset() -> String {
    match std::env::var("FALCON_DATASET") {
        Ok(s) if !s.is_empty() => s,
        _ => "rpool/falcon".to_string(),
    }
}

fn libnet_retry<F>(f: F) -> Result<(), Error>
where
    F: Fn() -> Result<(), libnet::Error>,
{
    for _ in 0..30 {
        if f().is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(f()?)
}

async fn find_propolis_port_in_log(
    logfile: String,
) -> Result<u16, anyhow::Error> {
    let timeout = Instant::now() + Duration::from_secs(10);
    let port =
        tokio::time::timeout_at(timeout, do_find_propolis_port_in_log(logfile))
            .await?
            .map_err(|e| {
                anyhow::anyhow!(
                    "timed out waiting to find propolis port in its log: {e}"
                )
            })?;
    Ok(port)
}

async fn do_find_propolis_port_in_log(
    logfile: String,
) -> Result<u16, anyhow::Error> {
    let re = regex::Regex::new(r#""local_addr":"\[::1?\]:([0-9]+)""#).unwrap();
    let mut reader = BufReader::new(File::open(&logfile).await?);
    let mut lines = reader.lines();
    loop {
        match lines.next_line().await? {
            Some(line) => {
                if let Some(cap) = re.captures(&line) {
                    // unwrap on get(1) should be ok, since captures() returns
                    // `None` if there are no matches found
                    let port = cap.get(1).unwrap();
                    let result = port.as_str().parse::<u16>()?;
                    return Ok(result);
                }
            }
            None => {
                sleep(Duration::from_millis(10)).await;

                // We might have gotten a partial line; close the file, reopen
                // it, and start reading again from the beginning.
                reader = BufReader::new(File::open(&logfile).await?);
                lines = reader.lines();
            }
        }
    }
}

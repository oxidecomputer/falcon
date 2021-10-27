// Copyright 2021 Oxide Computer Company

mod test;
mod util;

pub mod cli;
pub mod error;

use error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;

/// A Deployment is the top level Falcon object. It contains a set of nodes and
/// links that are logically namespaced under the name of the deployment. Links
/// interconnect nodes forming a network.
pub struct Deployment {
    /// The name of this deployment
    pub name: String,

    /// The nodes of this deployment
    pub nodes: Vec<Node>,

    /// The point to point links of this deployment interconnectiong nodes
    pub links: Vec<Link>,

    /// If persistent is set to true, this deployment will not autodestruct when
    /// dropped.
    pub persistent: bool,
}

/// A node in a falcon network.
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
pub struct Mount {
    /// Directory from host to mount.
    pub source: String,

    /// Directory in node to mount to.
    pub destination: String,
}

/// Node references are passed back to clients when nodes are created. These are
/// an opaque handle that can be used in conjunction with various methods
/// provided by the Deployment implementation.
#[derive(Copy, Clone)]
pub struct NodeRef {
    /// The index of the referenced node in `Deployment::nodes`
    index: usize,
}

/// Links connect nodes through a pair of Endpoints. Links are strictly point to
/// point. They are meant to represent a single cable between machines. The only
/// future exception to this may be for breakout cables that have a 1 to N
/// fanout.
pub struct Link {
    pub endpoints: [Endpoint; 2],
}

/// Endpoints are owned by a Link and reference nodes through a references.
pub struct Endpoint {
    /// The node this endpiont is attached to
    node: NodeRef,

    /// The link index within the referenced node e.g., if this is the 3rd link
    /// in the referenzed node index=2.
    index: usize,
}

/// Opaque handle to a link. Used by clients to perform API functions on
/// links owned by a Deployment.
#[derive(Copy, Clone)]
pub struct LinkRef {
    /// The index of the referenced link in `Deployment::links`
    _index: usize,
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
            persistent: false,
        }
    }

    /// Create a new node within this deployment with the given name. Names must
    /// conform to [A-Za-z]?[A-Za-z0-9_]*
    pub fn node(&mut self, name: &str, image: &str) -> NodeRef {
        namecheck!(name, "node");

        let id = uuid::Uuid::new_v4();

        let r = NodeRef {
            index: self.nodes.len(),
        };
        let n = Node {
            name: String::from(name),
            image: String::from(image),
            radix: 0,
            mounts: Vec::new(),
            id,
        };
        self.nodes.push(n);
        r
    }

    /// Create a new link within this deployment between the referenced nodes.
    pub fn link(&mut self, a: NodeRef, b: NodeRef) -> LinkRef {
        let r = LinkRef {
            _index: self.links.len(),
        };
        let l = Link {
            endpoints: [
                Endpoint {
                    node: a,
                    index: self.nodes[a.index].radix,
                },
                Endpoint {
                    node: b,
                    index: self.nodes[b.index].radix,
                },
            ],
        };
        self.links.push(l);
        self.nodes[a.index].radix += 1;
        self.nodes[b.index].radix += 1;
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

        self.nodes[n.index].mounts.push(Mount {
            source: cpath_str.to_string(),
            destination: dst.as_ref().to_string(),
        });

        Ok(())
    }

    /// Launch the deployment. This will first create the ZFS pool, followed
    /// by all of the links, then the nodes with endpoints on the specificed 
    /// links.
    pub fn launch(&self) -> Result<(), Error> {
        self.preflight()?;
        match self.do_launch() {
            Ok(()) => Ok(()),
            Err(e) => {
                println!("launch failed: {}", e);
                Err(e)
            }
        }
    }

    pub fn preflight(&self) -> Result<(), Error> {
        Ok(fs::create_dir_all(".falcon")?)
    }

    // TODO in parallel
    fn do_launch(&self) -> Result<(), Error> {

        println!("creating links");
        for l in self.links.iter() {
            l.create(&self)?;
        }

        println!("creating nodes");
        for n in self.nodes.iter() {
            n.launch(&self)?;
        }

        Ok(())
    }

    /// Tear down all the nodes, followed by the links and the ZFS pool
    // TODO in parallel
    pub fn destroy(&self) -> Result<(), Error> {

        for n in self.nodes.iter() {
            n.destroy(&self)?;
        }

        for l in self.links.iter() {
            l.destroy(&self)?;
        }

        Ok(())
    }

    /// Run a command synchronously in the vm.
    pub fn exec(&self, _n: NodeRef, _cmd: &str) -> Result<String, Error> {
        //TODO
        Err(Error::NotImplemented)
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

        let _name = self.nodes[n.index].node_name(self);
        let _cmd = cmd.to_string();

        thread::spawn(move || {
            //TODO
        });

        rx
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

impl Drop for Deployment {
    fn drop(&mut self) {
        if !self.persistent {
            match self.destroy() {
                Ok(()) => {}
                Err(e) => println!("cleanup failed: {}", e),
            }
        }
    }
}

impl Node {
    fn preflight(&self) -> Result<(), Error> {
        //TODO
        Err(Error::NotImplemented)
    }

    fn launch(&self, d: &Deployment) -> Result<(), Error> {
        self.preflight()?;

        let _node_name = self.node_name(d);

        let mut links: Vec<String> = Vec::new();
        for l in d.links.iter() {
            for e in l.endpoints.iter() {
                if d.nodes[e.node.index].name == self.name {
                    links.push(d.vnic_link_name(e));
                }
            }
        }

        // TODO launch propolis instance
        Err(Error::NotImplemented)
    }

    fn node_name(&self, d: &Deployment) -> String {
        format!("{}_{}", d.name, self.name)
    }

    fn destroy(&self, d: &Deployment) -> Result<(), Error> {
        let _node_name = self.node_name(d);

        // TODO destroy propolis node
        Err(Error::NotImplemented)
    }
}

impl Link {
    fn create(&self, d: &Deployment) -> Result<(), Error> {

        // create interfaces
        for e in self.endpoints.iter() {
            let slink = d.simnet_link_name(e);
            let vlink = d.vnic_link_name(e);

            let slink_h = netadm_sys::LinkHandle::Name(slink.clone());
            let vlink_h = netadm_sys::LinkHandle::Name(vlink.clone());

            // if dangling links exists, remove them
            println!("destroying link {}", &vlink);
            netadm_sys::delete_link(&vlink_h, netadm_sys::LinkFlags::Active)?;
            println!("destroying link {}", &slink);
            netadm_sys::delete_link(&slink_h, netadm_sys::LinkFlags::Active)?;

            println!("creating simnet link '{}'", &slink);
            netadm_sys::create_simnet_link(
                &slink, netadm_sys::LinkFlags::Active)?;

            println!("creating vnic link '{}'", &vlink);
            netadm_sys::create_vnic_link(
                &vlink, &slink_h, netadm_sys::LinkFlags::Active)?;

            println!("link pair created");
        }

        // make point to point connection beteween interfaces
        let slink0 = d.simnet_link_name(&self.endpoints[0]);
        let slink1 = d.simnet_link_name(&self.endpoints[1]);
        let slink0_h = netadm_sys::LinkHandle::Name(slink0);
        let slink1_h = netadm_sys::LinkHandle::Name(slink1);
        netadm_sys::connect_simnet_peers(&slink0_h, &slink1_h)?;

        Ok(())
    }

    fn destroy(&self, d: &Deployment) -> Result<(), Error> {

        for e in self.endpoints.iter() {
            let slink = d.simnet_link_name(e);
            let vlink = d.vnic_link_name(e);
            let slink_h = netadm_sys::LinkHandle::Name(slink.clone());
            let vlink_h = netadm_sys::LinkHandle::Name(vlink.clone());

            println!("destroying link {}", &vlink);
            netadm_sys::delete_link(&vlink_h, netadm_sys::LinkFlags::Active)?;
            println!("destroying link {}", &slink);
            netadm_sys::delete_link(&slink_h, netadm_sys::LinkFlags::Active)?;
        }

        Ok(())
    }
}

// Copyright 2021 Oxide Computer Company

mod dladm;
mod error;
mod test;
mod util;
mod zones;

pub mod cli;

use error::Error;
use std::fs;
use std::path::PathBuf;
use zfs_core::Zfs;

/// A Deployment is the top level Falcon object. It contains a set of zones and
/// links that are logically namespaced under the name of the deployment. Links
/// interconnect zones forming a network.
pub struct Deployment {
    /// The name of this deployment
    pub name: String,

    /// The nodes of this deployment implemented as zones
    pub zones: Vec<Zone>,

    /// The point to point links of this deployment interconnectiong zones
    pub links: Vec<Link>,

    /// If persistent is set to true, this deployment will not autodestruct when
    /// dropped.
    pub persistent: bool,
}

/// Zones make up the nodes of a Falcon network.
pub struct Zone {
    /// The name of the zone
    pub name: String,

    /// The zone brand
    pub brand: ZoneBrand,

    /// How many links the zone has
    pub radix: usize,

    pub mounts: Vec<Mount>,
}

/// Directories mounted from host machine into a zone.
pub struct Mount {
    /// Directory from host to mount.
    pub source: String,

    /// Directory in zone to mount to.
    pub destination: String,
}

/// The set of supported zone brands are captured by this enum
pub enum ZoneBrand {
    /// A copy of the host OS where software is linked to the global zone
    Lipkg,

    /// A zone type for runing Bhyve virtual machines
    Bhyve(String),
}

/// Zone references are passed back to clients when zones are created. These are
/// an opaque handle that can be used in conjunction with various methods
/// provided by the Deployment implementation.
#[derive(Copy, Clone)]
pub struct ZoneRef {
    /// The index of the referenced zone in `Deployment::zones`
    index: usize,
}

/// Links connect zones through a pair of Endpoints. Links are strictly point to
/// point. They are meant to represent a single cable between machines. The only
/// future exception to this may be for breakout cables that have a 1 to N
/// fanout.
pub struct Link {
    pub endpoints: [Endpoint; 2],
}

/// Endpoints are owned by a Link and reference zones through a references.
pub struct Endpoint {
    /// The zone this endpiont is attached to
    zone: ZoneRef,

    /// The link index within the referenced zone e.g., if this is the 3rd link
    /// in the referenzed zone index=2.
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
            zones: Vec::new(),
            links: Vec::new(),
            persistent: false,
        }
    }

    /// Create a new zone within this deployment with the given name. Names must
    /// conform to [A-Za-z]?[A-Za-z0-9_]*
    pub fn zone(&mut self, name: &str) -> ZoneRef {
        namecheck!(name, "zone");

        let r = ZoneRef {
            index: self.zones.len(),
        };
        let z = Zone {
            name: String::from(name),
            brand: ZoneBrand::Lipkg,
            radix: 0,
            mounts: Vec::new(),
        };
        self.zones.push(z);
        r
    }

    /// Create a new link within this deployment between the referenced zones.
    pub fn link(&mut self, a: ZoneRef, b: ZoneRef) -> LinkRef {
        let r = LinkRef {
            _index: self.links.len(),
        };
        let l = Link {
            endpoints: [
                Endpoint {
                    zone: a,
                    index: self.zones[a.index].radix,
                },
                Endpoint {
                    zone: b,
                    index: self.zones[b.index].radix,
                },
            ],
        };
        self.links.push(l);
        self.zones[a.index].radix += 1;
        self.zones[b.index].radix += 1;
        r
    }

    pub fn mount(
        &mut self,
        src: impl AsRef<str>,
        dst: impl AsRef<str>,
        z: ZoneRef,
    ) -> Result<(), Error> {
        let pb = PathBuf::from(src.as_ref());
        let cpath = fs::canonicalize(&pb)?;
        let cpath_str = cpath
            .to_str()
            .ok_or(Error::PathError(format!("bad path: {}", src.as_ref())))?;

        self.zones[z.index].mounts.push(Mount {
            source: cpath_str.to_string(),
            destination: dst.as_ref().to_string(),
        });

        Ok(())
    }

    /// Launch the deployment. This will first create the ZFS pool, followed
    /// by all of the links, then the zones with endpoints on the specificed links.
    pub fn launch(&self) -> Result<(), Error> {
        match self.do_launch() {
            Ok(()) => Ok(()),
            Err(e) => {
                // best effort destroy
                match self.destroy() {
                    Ok(()) => {}
                    Err(e) => {
                        println!("cleanup failed: {}", e);
                        println!("manual zone/zfs/dladm cleanup may be needed");
                    }
                }
                // return source error
                Err(e)
            }
        }
    }

    // TODO in parallel
    fn do_launch(&self) -> Result<(), Error> {
        self.pool_create()?;

        for l in self.links.iter() {
            l.create(&self)?;
        }

        for z in self.zones.iter() {
            z.launch(&self)?;
        }

        Ok(())
    }

    /// Tear down all the zones, followed by the links and the ZFS pool
    // TODO in parallel
    pub fn destroy(&self) -> Result<(), Error> {
        for z in self.zones.iter() {
            z.destroy(&self)?;
        }

        for l in self.links.iter() {
            l.destroy(&self)?;
        }

        self.pool_destroy()?;

        Ok(())
    }

    /// Qualified name of the deployment. The falcon- prefix indicates in various
    /// other systems contexts such as zfs and zones that whatever the user is
    /// looking at originated from Falcon.
    fn qname(&self) -> String {
        format!("falcon-{}", self.name)
    }

    /// Each Deployment gets a zfs pool with the name `rpool/<qualified-name>`
    fn zfs_rpool_name(&self) -> String {
        format!("rpool/{}", self.qname())
    }

    /// Deployment pools are mounted at the root directory under the qualified
    /// name.
    fn path(&self) -> String {
        format!("/{}", self.qname())
    }

    /// Create a ZFS pool for the deployment
    fn pool_create(&self) -> Result<(), Error> {
        zones::create_zpool(self.zfs_rpool_name(), self.path())?;
        Ok(())
    }

    /// Destroy a deployments ZFS pool
    fn pool_destroy(&self) -> Result<(), Error> {
        let zfs = Zfs::new()?;
        let poolname = self.zfs_rpool_name();

        if zfs.exists(&poolname) {
            zfs.destroy(&poolname)?;
        }

        Ok(())
    }

    /// Run a command synchronously in the zone.
    pub fn exec(&self, z: ZoneRef, cmd: &str) -> Result<String, Error> {
        zones::run_command(&self.zones[z.index].zone_name(self), cmd)
    }

    /// Run a command asynchronously in the zone.
    pub fn spawn(&self, _z: ZoneRef, _cmd: &str) -> Result<String, Error> {
        Err(Error::NotImplemented)
    }

    /// Copy the debug target from this crate into the referenced container at
    /// the provided location.
    pub fn copy_debug_target(
        &self,
        _z: ZoneRef,
        _src: &str,
        _dest: &str,
    ) -> Result<String, Error> {
        Err(Error::NotImplemented)
    }

    fn simnet_link_name(&self, e: &Endpoint) -> String {
        format!(
            "{}_{}_sim{}",
            self.name, self.zones[e.zone.index].name, e.index,
        )
    }

    fn vnic_link_name(&self, e: &Endpoint) -> String {
        format!(
            "{}_{}_vnic{}",
            self.name, self.zones[e.zone.index].name, e.index,
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

impl Zone {
    fn preflight(&self) -> Result<(), Error> {
        match &self.brand {
            ZoneBrand::Lipkg => self.lipkg_preflight(),
            ZoneBrand::Bhyve(image) => self.bhyve_preflight(image),
        }
    }

    fn lipkg_preflight(&self) -> Result<(), Error> {
        zones::ensure_base_zone()
    }

    fn bhyve_preflight(&self, _image: &String) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    fn launch(&self, d: &Deployment) -> Result<(), Error> {
        self.preflight()?;

        let zone_name = self.zone_name(d);
        let zone_path = self.path(d);

        let mut links: Vec<String> = Vec::new();
        for l in d.links.iter() {
            for e in l.endpoints.iter() {
                if d.zones[e.zone.index].name == self.name {
                    links.push(d.vnic_link_name(e));
                }
            }
        }

        match &self.brand {
            ZoneBrand::Lipkg => zones::launch_lipkg_zone(
                zone_name.as_str(),
                zone_path.as_str(),
                &links,
                &self.mounts,
            ),

            ZoneBrand::Bhyve(image) => zones::launch_bhyve_zone(
                zone_name.as_str(),
                zone_path.as_str(),
                &links,
                image,
            ),
        }
    }

    fn zone_name(&self, d: &Deployment) -> String {
        format!("{}_{}", d.name, self.name)
    }

    fn path(&self, d: &Deployment) -> String {
        format!("{}/{}", d.path(), self.name)
    }

    fn destroy(&self, d: &Deployment) -> Result<(), Error> {
        let zone_name = self.zone_name(d);

        match zones::get_zone(zone_name.as_str()) {
            // If the zone does not exist, nothing to do
            Err(crate::Error::NotFound) => return Ok(()),
            Err(e) => return Err(e),
            Ok(z) => zones::destroy_zone(&z),
        }
    }
}

impl Link {
    fn create(&self, d: &Deployment) -> Result<(), Error> {
        let h = dladm::get_handle()?;

        // create interfaces
        for e in self.endpoints.iter() {
            let slink = d.simnet_link_name(e);
            let vlink = d.vnic_link_name(e);

            // if dangling links exists, remove them
            dladm::destroy_vnic_interface(&vlink, h)?;
            dladm::destroy_simnet_interface(&slink, h)?;

            let link_id = dladm::create_simnet_interface(&slink, h)?;
            dladm::create_vnic_interface(&vlink, link_id, h)?;
        }

        // make point to point connection beteween interfaces
        let slink0 = d.simnet_link_name(&self.endpoints[0]);
        let slink1 = d.simnet_link_name(&self.endpoints[1]);
        dladm::connect_simnet_interfaces(&slink0, &slink1, h)?;

        Ok(())
    }

    fn destroy(&self, d: &Deployment) -> Result<(), Error> {
        let h = dladm::get_handle()?;

        for e in self.endpoints.iter() {
            let slink = d.simnet_link_name(e);
            let vlink = d.vnic_link_name(e);

            dladm::destroy_vnic_interface(&vlink, h)?;
            dladm::destroy_simnet_interface(&slink, h)?;
        }

        Ok(())
    }
}

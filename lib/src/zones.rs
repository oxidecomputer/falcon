// Copyright 2021 Oxide Computer Company

use crate::error::Error;
use std::ffi::OsStr;
use std::io;
use zfs_core::Zfs;

pub(crate) static BASE_ZONE_NAME: &str = "falcon-base";

pub(crate) fn ensure_base_zone() -> Result<(), Error> {
    // get the base zone, if it does not exist create it
    match get_zone(BASE_ZONE_NAME) {
        Ok(_) => Ok(()),
        Err(Error::NotFound) => {
            println!("base zone not found");
            create_base_zone()?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub(crate) fn get_zone(name: &str) -> Result<zone::Zone, Error> {
    let zones = zone::Adm::list()?;

    for z in zones.iter() {
        if z.name() == name {
            return Ok(z.clone());
        }
    }

    Err(Error::NotFound)
}

pub(crate) fn launch_lipkg_zone(
    name: &str,
    path: &str,
    links: &Vec<String>,
) -> Result<(), Error> {
    // configure

    let mut cfg =
        zone::Config::create(name, true, zone::CreationOptions::Default);
    cfg.get_global().set_path(path);

    for l in links.iter() {
        cfg.add_net(&zone::Net {
            physical: l.clone(),
            address: None,
            allowed_address: None,
            default_router: None,
        });
    }

    cfg.run()?;

    // clone from falcon base zone

    let mut zoneadm = zone::Adm::new(name);
    zoneadm.clone(BASE_ZONE_NAME)?;

    // boot
    zoneadm.boot()?;

    // wait for the network in the zone to come up
    let mut retries = 0;
    loop {
        let status = smf::Query::new().zone(name).get_status(
            smf::QuerySelection::ByPattern(&["svc:/milestone/network:default"]),
        );
        match status {
            Ok(status) => {
                let mut online = false;
                for s in status {
                    if s.state == smf::SmfState::Online {
                        online = true;
                        break;
                    }
                }
                if online {
                    break;
                }
            }
            Err(e) => {
                if retries >= 10 {
                    return Err(Error::QueryError(e));
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
        retries += 1
    }

    Ok(())
}

pub(crate) fn launch_bhyve_zone(
    _name: &str,
    _path: &str,
    _links: &Vec<String>,
    _image: &String,
) -> Result<(), Error> {
    Err(Error::NotImplemented)
}

pub(crate) fn destroy_zone(name: &str) -> Result<(), Error> {
    let mut zoneadm = zone::Adm::new(name);
    zoneadm.halt()?;
    zoneadm.uninstall(true)?;

    let mut cfg =
        zone::Config::create(name, true, zone::CreationOptions::Default);
    cfg.delete(true);
    cfg.run()?;

    Ok(())
}

pub(crate) fn create_zpool<S: AsRef<str>>(
    name: S,
    mount: S,
) -> Result<(), Error> {
    let zfs = Zfs::new()?;

    let mut props = nvpair::NvList::new();
    props.insert("mountpoint", mount.as_ref())?;
    match zfs.create(name.as_ref(), zfs_core::DataSetType::Zfs, &props) {
        Ok(()) => Ok(()),

        Err(e) => match e.kind() {
            io::ErrorKind::AlreadyExists => Ok(()),
            _ => Err(e),
        },
    }?;

    Ok(())
}

fn create_base_zone() -> Result<(), Error> {
    create_zpool("rpool/falcon-base", "/falcon")?;

    let mut cfg = zone::Config::create(
        BASE_ZONE_NAME,
        true,
        zone::CreationOptions::Default,
    );

    println!("creating base zone");
    cfg.get_global().set_path("/falcon");
    cfg.run()?;

    let mut zoneadm = zone::Adm::new(BASE_ZONE_NAME);
    println!("installing base zone");
    zoneadm.install(&[])?;

    Ok(())
}

pub(crate) fn run_command(
    zone: impl AsRef<str>,
    cmd: impl AsRef<OsStr>,
) -> Result<String, Error> {
    let c = cmd.as_ref();

    let zlogin = zone::Zlogin::new(zone);
    Ok(zlogin
        .exec(c)
        .map_err(|e| Error::Exec(format!("{:?}: {}", c, e)))?)
}

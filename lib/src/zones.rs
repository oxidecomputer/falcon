// Copyright 2021 Oxide Computer Company

use crate::error::Error;
use fs2::FileExt;
use std::ffi::OsStr;
use std::fs;

pub(crate) static BASE_ZONE_NAME: &str = "falcon-base";

pub(crate) fn ensure_base_zone() -> Result<(), Error> {
    let lockfile = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open("/tmp/falcon-base-zone-init")?;
    lockfile.lock_exclusive()?;

    // get the base zone, if it does not exist create it
    let result = match get_zone(BASE_ZONE_NAME) {
        Ok(_) => Ok(()),
        Err(e) => match e {
            Error::NotFound => {
                create_base_zone()?;
                Ok(())
            }
            _ => Err(e),
        },
    };

    lockfile.unlock()?;
    result
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
    mounts: &Vec<crate::Mount>,
) -> Result<(), Error> {
    let mut cfg = zone::Config::create(name, true, zone::CreationOptions::Default);
    cfg.get_global().set_brand("lipkg");
    cfg.get_global().set_path(path);
    cfg.get_global().set_ip_type(zone::IpType::Exclusive);

    for l in links.iter() {
        cfg.add_net(&zone::Net {
            physical: l.clone(),
            address: None,
            allowed_address: None,
            default_router: None,
        });
    }

    for m in mounts.iter() {
        cfg.add_fs(&zone::Fs {
            ty: "lofs".to_string(),
            special: m.source.clone(),
            dir: m.destination.clone(),
            ..Default::default()
        });
    }

    cfg.run()
        .map_err(|e| Error::Wrap(format!("configure zone: {}", e)))?;

    // clone from falcon base zone
    let mut zoneadm = zone::Adm::new(name);
    zoneadm
        .clone(BASE_ZONE_NAME)
        .map_err(|e| Error::Wrap(format!("clone base zone: {}", e)))?;

    // boot
    zoneadm
        .boot()
        .map_err(|e| Error::Wrap(format!("boot zone: {}", e)))?;

    // wait for the network in the zone to come up
    let mut retries = 0;
    loop {
        let status = smf::Query::new()
            .zone(name)
            .get_status(smf::QuerySelection::ByPattern(&[
                "svc:/milestone/network:default",
            ]));
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
        std::thread::sleep(std::time::Duration::from_secs(5));
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

pub(crate) fn destroy_zone(zone: &zone::Zone) -> Result<(), Error> {
    let mut zoneadm = zone::Adm::new(zone.name());
    let mut cfg = zone::Config::create(zone.name(), true, zone::CreationOptions::Default);

    match zone.state() {
        zone::State::Configured => {
            cfg.delete(true);
            cfg.run()?;
        }
        zone::State::Incomplete => {
            cfg.delete(true);
            cfg.run()?;
        }
        zone::State::Installed => {
            zoneadm.uninstall(true)?;
            cfg.delete(true);
            cfg.run()?;
        }
        zone::State::Ready => {
            zoneadm.uninstall(true)?;
            cfg.delete(true);
            cfg.run()?;
        }
        zone::State::Mounted => {
            zoneadm.uninstall(true)?;
            cfg.delete(true);
            cfg.run()?;
        }
        zone::State::Running => {
            zoneadm.halt()?;
            zoneadm.uninstall(true)?;
            cfg.delete(true);
            cfg.run()?;
        }
        zone::State::ShuttingDown => {
            zoneadm.halt()?;
            zoneadm.uninstall(true)?;
            cfg.delete(true);
            cfg.run()?;
        }
        zone::State::Down => {
            zoneadm.uninstall(true)?;
            cfg.delete(true);
            cfg.run()?;
        }
    }

    Ok(())
}

pub(crate) fn destroy_zpool<S: AsRef<str>>(name: S) -> Result<(), Error> {
    // TODO XXX: i truely hate this, rust != perl `std::process::Command` should
    // never be used. However, the zfs crates that exist do not expose
    // sufficient functionality and I do not have the bandwidth to create a
    // libzfs crate.
    println!("destroying zfs pool {}", name.as_ref());
    let mut command = std::process::Command::new("zfs");
    let cmd = command.args(&["destroy", "-r", name.as_ref()]);
    cmd.output()?;
    Ok(())
}

pub(crate) fn create_zpool<S: AsRef<str>>(name: S, mount: S) -> Result<(), Error> {
    println!("clearing out any existing {}", mount.as_ref());
    // clear out target directory
    match fs::remove_dir_all(mount.as_ref()) {
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {}
            _ => return Err(Error::IO(e)),
        },
        Ok(_) => {}
    }

    /*

    XXX: this crate does not have zfs_mount and a bunch of other things
    needed to make this reliable

    let zfs = Zfs::new()?;

    let mut props = nvpair::NvList::new();
    props.insert("mountpoint", mount.as_ref())?;

    match zfs.create(name.as_ref(), zfs_core::DataSetType::Zfs, &props) {
        Ok(_) => {},
        Err(e) => match e.kind() {
            io::ErrorKind::AlreadyExists => {},
            _ => return Err(Error::Zfs(
                    format!("create zpool: name={} mount={}: {}",
                        name.as_ref(), mount.as_ref(), e))
            ),
        },
    };

    // wait for zfs pool to exist
    let mut retries = 0;
    loop {
        if zfs.exists(name.as_ref()) {
            break;
        }
        sleep(Duration::from_secs(1));
        retries = retries + 1;
        if retries >= 10 {
            return Err(Error::Zfs(
                    format!("zpool creation timeout: name={} mount={}",
                        name.as_ref(), mount.as_ref())))
        }
    }

    // wait for zfs pool to be mounted

    let mount_path = path::Path::new(mount.as_ref());
    retries = 0;
    loop {
        if retries >= 10 {
            return Err(Error::Zfs(
                    format!("zpool mount timeout: name={} mount={}",
                        name.as_ref(), mount.as_ref())))
        }
        if !mount_path.is_dir() {
            retries = retries + 1;
            sleep(Duration::from_secs(1));
            continue;
        }
        let parent = match mount_path.parent() {
            Some(p) => p,
            None => {
                retries = retries + 1;
                sleep(Duration::from_secs(1));
                continue;
            }
        };
        let mount_path_metadata = match fs::metadata(mount_path) {
            Ok(m) => m,
            Err(_) => {
                retries = retries + 1;
                sleep(Duration::from_secs(1));
                continue;
            }
        };

        let parent_metadata = match fs::metadata(parent) {
            Ok(m) => m,
            Err(_) => {
                retries = retries + 1;
                sleep(Duration::from_secs(1));
                continue;
            }
        };

        if parent_metadata.dev() != mount_path_metadata.dev() {
            break;
        }
        retries = retries + 1;
        sleep(Duration::from_secs(1));
    }
    */

    // TODO XXX: i truely hate this, rust != perl `std::process::Command` should
    // never be used. However, the zfs crates that exist do not expose
    // sufficient functionality and I do not have the bandwidth to create a
    // libzfs crate.
    println!("creating zfs pool {}", name.as_ref());
    let mut command = std::process::Command::new("zfs");
    let cmd = command.args(&[
        "create",
        "-o",
        &format!("mountpoint={}", mount.as_ref()),
        name.as_ref(),
    ]);
    cmd.output()?;

    Ok(())
}

fn create_base_zone() -> Result<(), Error> {
    println!("creating base zone zpool");
    create_zpool("rpool/falcon-base", "/falcon")?;

    println!("creating base zone");
    let mut cfg = zone::Config::create(BASE_ZONE_NAME, true, zone::CreationOptions::Default);

    //let mut lpriv = std::collections::BTreeSet::new();
    //lpriv.insert("default".to_string());

    cfg.get_global().set_path("/falcon");
    cfg.get_global().set_brand("lipkg");
    //cfg.get_global().set_limitpriv(lpriv);
    cfg.get_global().set_ip_type(zone::IpType::Exclusive);
    cfg.run()?;

    println!("installing base zone");
    let mut zoneadm = zone::Adm::new(BASE_ZONE_NAME);
    zoneadm.install(&[])?;
    println!("base zone installed");

    Ok(())
}

pub(crate) fn run_command(zone: impl AsRef<str>, cmd: impl AsRef<OsStr>) -> Result<String, Error> {
    let c = cmd.as_ref();

    let zlogin = zone::Zlogin::new(zone);
    Ok(zlogin
        .exec(c)
        .map_err(|e| Error::Exec(format!("{:?}: {}", c, e)))?)
}

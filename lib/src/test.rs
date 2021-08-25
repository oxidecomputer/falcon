// Copyright 2021 Oxide Computer Company

#[cfg(test)]
mod test {

    use anyhow::{anyhow, Result};
    use std::ptr;
    use zfs_core::Zfs;

    /// Test that when an empty deployment is launched the correct ZFS pools get
    /// created and when a deployment is destroyd the associated zfs pools are
    /// destroyed.
    #[test]
    fn empty_launch() -> Result<()> {
        let d = crate::Deployment::new("empty-launch");
        d.launch()?;

        // verify zfs pool exists
        let zfs = Zfs::new()?;
        assert_eq!(zfs.exists(d.zfs_rpool_name()), true);

        // verify base zone exists
        crate::zones::get_zone(crate::zones::BASE_ZONE_NAME)?;

        d.destroy()?;

        // verify base zone still exists
        crate::zones::get_zone(crate::zones::BASE_ZONE_NAME)?;

        // verify zfs pool does not exist
        assert_eq!(zfs.exists(d.zfs_rpool_name()), false);

        Ok(())
    }

    /// Test that when a single zone deployment gets launched, the corresponding
    /// zone is created and when the deployment is destroyed, the corresponding
    /// zone is also destroyed.
    #[test]
    fn solo_launch() -> Result<()> {
        let mut d = crate::Deployment::new("solo");
        let z = d.zone("violin");

        // mount a file into the zone
        let some_data = "some data";
        std::fs::write("/tmp/some_data", some_data)?;
        d.mount("/tmp/some_data", "/opt/some_data", z)?;

        d.launch()?;

        // verify zone exists
        crate::zones::get_zone("solo_violin")?;

        // run a command in the zone
        let some_mounted_data = d.exec(z, "cat /opt/some_data")?;

        d.destroy()?;

        // verify zone does not exist
        match crate::zones::get_zone("solo_violin") {
            Ok(_) => return Err(anyhow!("solo_violin zone should be gone")),
            Err(crate::Error::NotFound) => {}
            Err(e) => return Err(anyhow!("{}", e)),
        }

        // check the mounted data
        assert_eq!(some_data, some_mounted_data);

        Ok(())
    }

    /// Test that when a two zone deployment gets launched, the corresponding
    /// simnet and vnic links get created and destroyed.
    #[test]
    fn duo_launch() -> Result<()> {
        let mut h: *mut crate::dladm::dladm_handle = ptr::null_mut();
        let status = unsafe { crate::dladm::dladm_open(&mut h) };
        if status != crate::dladm::dladm_status_t_DLADM_STATUS_OK {
            return Err(anyhow!("test: get dladm handle"));
        }

        // These are the links we'll expect to see, one simnet and one vnic for
        // each zone
        let links = [
            String::from("duo_violin_sim0"),
            String::from("duo_violin_vnic0"),
            String::from("duo_piano_sim0"),
            String::from("duo_piano_vnic0"),
        ];

        let mut d = crate::Deployment::new("duo");
        let violin = d.zone("violin");
        let piano = d.zone("piano");
        d.link(violin, piano);

        d.launch()?;

        // set ipv6 link local addresses
        println!("VIOLIN DLADM\n{}\n", d.exec(violin, "dladm")?);
        d.exec(violin, "ipadm create-addr -T addrconf duo_violin_vnic0/v6")?;
        println!("VIOLIN IPADM\n{}\n", d.exec(violin, "ipadm")?);

        println!("PIANO DLADM\n{}\n", d.exec(piano, "dladm")?);
        d.exec(piano, "ipadm create-addr -T addrconf duo_piano_vnic0/v6")?;
        println!("PIANO IPADM\n{}\n", d.exec(piano, "ipadm")?);

        // get piano addresses
        let piano_addr =
            d.exec(piano, "ipadm show-addr -p -o ADDR duo_piano_vnic0/v6")?;

        // wait for piano address to become ready
        let mut retries = 0;
        loop {
            let state =
                d.exec(piano, "ipadm show-addr -po state duo_piano_vnic0/v6")?;
            if state == "ok" {
                break;
            }
            retries += 1;
            if retries >= 10 {
                return Err(anyhow!(
                    "timed out waiting for duo_piano_vnic0/v6"
                ));
            }
            std::thread::sleep(std::time::Duration::from_secs(1))
        }

        // do a ping
        let ping_cmd =
            format!("ping {} 1", piano_addr.strip_suffix("/10").unwrap());
        d.exec(violin, ping_cmd.as_str())?;

        // verify links exist
        for l in links.iter() {
            crate::dladm::link_id(l, h)?;
        }

        // This does a d.destroy() call
        drop(d);

        // verify links to not exist
        for l in links.iter() {
            check_link_absent(l, h)?;
        }

        Ok(())
    }

    fn check_link_absent(
        name: &String,
        h: *mut crate::dladm::dladm_handle,
    ) -> Result<()> {
        match crate::dladm::link_id(name, h) {
            Ok(_) => return Err(anyhow!("link {} should be gone", name)),
            Err(crate::Error::Dladm(
                _,
                crate::dladm::dladm_status_t_DLADM_STATUS_NOTFOUND,
            )) => Ok(()),
            Err(e) => return Err(anyhow!("{}", e)),
        }
    }
}

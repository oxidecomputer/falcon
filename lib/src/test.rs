// Copyright 2021 Oxide Computer Company

#[cfg(test)]
mod test {

    use anyhow::{anyhow, Result};

    /// Test that when an empty deployment is launched the correct ZFS pools get
    /// created and when a deployment is destroyd the associated zfs pools are
    /// destroyed.
    #[tokio::test]
    async fn empty_launch() -> Result<()> {
        let mut d = crate::Runner::new("empty-launch");
        d.persistent = true;
        d.launch().await?;
        d.destroy()?;

        Ok(())
    }

    /// Test that when a single node deployment gets launched, the corresponding
    /// node is created and when the deployment is destroyed, the corresponding
    /// node is also destroyed.
    #[tokio::test]
    async fn solo_launch() -> Result<()> {
        let mut d = crate::Runner::new("solo");
        let z = d.node("violin", "helios-1.0", 1, 1024);

        // mount a file into the node
        let some_data = "some data";
        std::fs::create_dir_all("/tmp/solo")?;
        std::fs::write("/tmp/solo/some_data", some_data)?;
        d.mount("/tmp/solo", "/opt/solo", z)?;

        d.launch().await?;

        // run a command on the node
        let some_mounted_data = d.exec(z, "cat /opt/solo/some_data").await?;

        d.persistent = true;
        d.destroy()?;

        // check the mounted data
        assert_eq!(some_data, some_mounted_data);

        Ok(())
    }

    /// Test that when a two node deployment gets launched, the corresponding
    /// simnet and vnic links get created and destroyed.
    #[tokio::test]
    async fn duo_launch() -> Result<()> {
        // These are the links we'll expect to see, one simnet and one vnic for
        // each node
        let links = [
            String::from("duo_violin_sim0"),
            String::from("duo_violin_vnic0"),
            String::from("duo_piano_sim0"),
            String::from("duo_piano_vnic0"),
        ];

        let mut d = crate::Runner::new("duo");
        let violin = d.node("violin", "helios-1.0", 1, 1024);
        let piano = d.node("piano", "helios-1.0", 1, 1024);
        d.link(violin, piano);

        d.launch().await?;

        // set ipv6 link local addresses
        println!("VIOLIN DLADM\n{}\n", d.exec(violin, "dladm show-link").await?);
        d.exec(
            violin,
            "ipadm create-addr -t -T addrconf duo_violin_vnic0/v6",
        ).await?;
        println!("VIOLIN IPADM\n{}\n", d.exec(violin, "ipadm show-addr").await?);

        println!("PIANO DLADM\n{}\n", d.exec(piano, "dladm show-link").await?);
        d.exec(piano, "ipadm create-addr -t -T addrconf duo_piano_vnic0/v6").await?;
        println!("PIANO IPADM\n{}\n", d.exec(piano, "ipadm show-addr").await?);

        // get piano addresses
        let piano_addr = d.exec(piano, "ipadm show-addr -p -o ADDR duo_piano_vnic0/v6").await?;

        // wait for piano address to become ready
        let mut retries = 0;
        loop {
            let state = d.exec(piano, "ipadm show-addr -po state duo_piano_vnic0/v6").await?;
            if state == "ok" {
                break;
            }
            retries += 1;
            if retries >= 10 {
                return Err(anyhow!("timed out waiting for duo_piano_vnic0/v6"));
            }
            std::thread::sleep(std::time::Duration::from_secs(1))
        }

        // do a ping
        let ping_cmd = format!("ping {} 1", piano_addr.strip_suffix("/10").unwrap());
        d.exec(violin, ping_cmd.as_str()).await?;

        // verify links exist
        for l in links.iter() {
            //crate::dladm::link_id(l, h)?;
            let h = netadm_sys::LinkHandle::Name(l.clone());
            netadm_sys::get_link(h.id()?)?;
        }

        // This does a d.destroy() call
        drop(d);

        // verify links to not exist
        for l in links.iter() {
            check_link_absent(l)?;
        }

        Ok(())
    }

    fn check_link_absent(name: &String) -> Result<()> {
        let h = netadm_sys::LinkHandle::Name(name.clone());
        match h.id() {
            Ok(_) => return Err(anyhow!("link {} should be gone", name)),
            Err(netadm_sys::Error::NotFound(_)) => return Ok(()),
            Err(e) => return Err(anyhow!("{}", e)),
        }
    }
}

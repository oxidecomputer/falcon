// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

#[cfg(test)]
mod tests {
    use anyhow::{anyhow, Result};
    use libfalcon::{unit::gb, Runner};

    #[tokio::test]
    #[ignore]
    async fn duo_ping() -> Result<()> {
        let mut d = Runner::new("duo");
        let violin = d.node("violin", "helios-2.3", 2, 2048);
        let piano = d.node("piano", "helios-2.3", 2, gb(2));
        d.link(violin, piano);

        d.launch().await?;

        // set ipv6 link local addresses
        d.exec(violin, "ipadm create-addr -t -T addrconf vioif0/v6")
            .await?;
        d.exec(piano, "ipadm create-addr -t -T addrconf vioif0/v6")
            .await?;

        // get piano addresses
        let piano_addr = d
            .exec(piano, "ipadm show-addr -p -o ADDR vioif0/v6")
            .await?;

        // wait for piano address to become ready
        let mut retries = 0;
        loop {
            let state =
                d.exec(piano, "ipadm show-addr -po state vioif0/v6").await?;
            if state == "ok" {
                break;
            }
            retries += 1;
            if retries >= 10 {
                return Err(anyhow!("timed out waiting for vioif0/v6"));
            }
            std::thread::sleep(std::time::Duration::from_secs(1))
        }

        // do a ping
        let ping_cmd =
            format!("ping {} 1", piano_addr.strip_suffix("/10").unwrap());
        d.exec(violin, ping_cmd.as_str()).await?;

        Ok(())
    }
}

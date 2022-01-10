# FALCON

**_Fast Assessment Laboratory for Computers On Networks_**

Falcon is a Rust API for creating network topologies composed of zones
interconnected by simnet links. It's designed to be used for both automated
testing and as a development environment for networked systems.

**Falcon runs on Helios >= 1.0.20707**

Currently the nightly toolchain is required.

## Using from Rust tests

This workflow is useful for automated testing.

```Rust
#[cfg(test)]
mod tests {
    use anyhow::{anyhow, Result};
    use libfalcon::{Runner, unit::gb};

    #[tokio::test]
    #[ignore]
    async fn duo_ping() -> Result<()> {
        let mut d = Runner::new("duo");
        let violin = d.node("violin", "helios", 2, 2048);
        let piano = d.node("piano", "helios", 2, gb(2));
        d.link(violin, piano);

        d.launch().await?;

        // set ipv6 link local addresses
        d.exec(
            violin,
            "ipadm create-addr -t -T addrconf duo_violin_vnic0/v6",
        ).await?;
        d.exec(piano, "ipadm create-addr -t -T addrconf duo_piano_vnic0/v6").await?;

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

        Ok(())
    }
}
```

[Working example](examples/duo-unit).

## Using from command line

This workflow is useful for actively developing networked systems.

### Describe the topology

```Rust
use libfalcon::{cli::{run, RunMode}, error::Error, Runner, unit::gb};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("duo");

    // nodes
    let violin = d.node("violin", "helios", 2, 2048);
    let piano = d.node("piano", "helios", 2, gb(2));


    d.mount("./cargo-bay", "/opt/stuff", violin)?;
    d.mount("./cargo-bay", "/opt/stuff", piano)?;

    // links
    d.link(violin, piano);

    match run(&mut d).await? {
        RunMode::Launch => {
            d.exec(violin, "ipadm create-addr -t -T addrconf vioif0/v6").await?;
            d.exec(piano,  "ipadm create-addr -t -T addrconf vioif0/v6").await?;
            Ok(())
        }
        _ => { Ok(()) }
    }
}
```

### Launch the topology

```shell
pfexec cargo run launch
```

### Do some work

```shell
pfexec zlogin duo_violin
...
```

### Destroy the topology

```shell
pfexec cargo run destroy
```


[Working example](examples/duo).

## Building

Note that running the tests for the first time will take a while as a new lipkg
zone needs to be installed. On my machine this is about 6-7 minutes.

```
cargo build
pfexec cargo test
pfexec cargo test -- --ignored
```

### Package Dependencies

```shell
pkg install \
    pkg:/system/zones/brand/ipkg \
    pkg:/system/zones/brand/sparse \
    pkg:/ooce/developer/clang-110
```

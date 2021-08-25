# FALCON

**_Fast Assessment Laboratory for Computers On Networks_**

Falcon is a Rust API for creating network topologies composed of zones
interconnected by simnet links. It's designed to be used for both automated
testing and as a development environment for networked systems.

**For Falcon to work, you will need to be running a libdladm with this patch**
- https://code.illumos.org/c/illumos-gate/+/1668

## Using from Rust tests

This workflow is useful for automated testing.

```Rust
#[cfg(test)]
mod tests {
    use anyhow::{anyhow, Result};

    #[test]
    #[ignore]
    fn duo_ping() -> Result<()> {
        let mut d = libfalcon::Deployment::new("duo");
        let violin = d.zone("violin");
        let piano = d.zone("piano");
        d.link(violin, piano);

        d.launch()?;

        // set ipv6 link local addresses
        d.exec(violin, "ipadm create-addr -T addrconf duo_violin_vnic0/v6")?;
        d.exec(piano, "ipadm create-addr -T addrconf duo_piano_vnic0/v6")?;

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

        Ok(())
    }
}
```

[Working example](examples/duo-unit).

## Using from command line

This workflow is useful for actively developing networked systems.

### Describe the topology

```Rust
use libfalcon::{cli::run, Deployment};

fn main() {

    let mut d = Deployment::new("duo");

    // nodes
    let violin = d.zone("violin");
    let piano = d.zone("piano");

    // links
    d.link(violin, piano);

    run(&mut d);

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

Falcon depends on having the illumos source available. This is because libdladm
is currently considered a private library and the headers are not included in a 
normal illumos install. The `ILLUMOS_SRC` environment variable must point to a
local [illumos-gate](https://github.com/illumos/illumos-gate) repo.

```
export ILLUMOS_SRC=<path-to-illumos-gate-source> 
cargo build
pfexec cargo test
pfexec cargo test -- --ignored

```

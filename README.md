# FALCON

**_Fast Assessment Laboratory for Computers On Networks_**

Falcon is a Rust API for creating network topologies composed of 
[Propolis](https://github.com/oxidecomputer/propolis) VMs interconnected by
simnet links. It's designed to be used for both automated testing and as a
development environment for networked systems.

**Falcon runs on Helios >= 1.0.20707**

Currently the nightly toolchain is required.

## Installing

Install `propolis-server` from the
[falcon branch](https://github.com/oxidecomputer/propolis/tree/falcon).
The`get-propolis.sh` script can also be used to automatically install
propolis-server form the current falcon CI build.

Set up propolis, firmware and OS base images.
```
./get-propolis.sh
./get-ovmf.sh
./setup-base-images.sh
```

## QuickStart

To get a ready-to-go Falcon project use the
[falcon-template](https://github.com/oxidecomputer/falcon-template).


```shell
cargo generate --git https://github.com/oxidecomputer/falcon-template --name duo
```

This will create a cargo project with the following topology.

```Rust
use libfalcon::{cli::run, error::Error, Runner, unit::gb};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("duo");

    // nodes, each with 2 cores and 2G of memory
    let violin = d.node("violin", "helios-1.1", 2, gb(2));
    let piano = d.node("piano", "helios-1.1", 2, gb(2));

    // links
    d.link(violin, piano);

    run(&mut d).await?;

    Ok(())
}
```

### Launch the topology

The following will launch the VMs in your topology and do some basic setup. When
the call returns, your topology is ready to use.

```shell
cargo build
pfexec ./target/debug/duo launch
```

### Get a serial connection to a node

Once the topology is up, you can access the nodes via serial connection. Tap the
enter key a few times after running the serial command below. To exit the
console use `ctl-q`.

```shell
./target/debug/duo serial violin
```

### Destroy the topology

```shell
pfexec ./target/debug/duo destroy
```

### Learn More

- The primary reference documentation is in the [wiki](https://github.com/oxidecomputer/falcon/wiki/Reference).
- [Working examples](examples).

## Building and testing

This assumes that that the instructions in the install section have been run.

```
cargo build
pfexec cargo test -- --test-threads 1
pfexec cargo test -- --test-threads 1 --ignored
```

Due to a shared `.falcon` directory, concurrent tests are not possible at the
current time.

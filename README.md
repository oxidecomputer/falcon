# FALCON

**_Fast Assessment Laboratory for Computers On Networks_**

Falcon is a Rust API for creating network topologies composed of 
[Propolis](https://github.com/oxidecomputer/propolis) VMs interconnected by
simnet links. It's designed to be used for both automated testing and as a
development environment for networked systems.

## Requirements

- Falcon runs on Helios >= 1.0.20707
- Falcon uses [propolis](https://github.com/oxidecomputer/propolis) which
  requires hardware virtualization support. Running Falcon on bare metal is
  recommended. While nested virt can be made to work, it often requires wizardry
  and is known to have flaky behaviors.

## Installing

Install `propolis-server`.  The`get-propolis.sh` script can also be used to
automatically install propolis-server form the current Falcon CI build.

Set up propolis, firmware and OS base images.
```
./get-propolis.sh
./get-ovmf.sh
./setup-base-images.sh
```

Falcon-enabled propolis builds are kicked out by Propolis CI. See
[this run](https://github.com/oxidecomputer/propolis/runs/18723647907)
as an example.

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

    // nodes
    let violin = d.node("violin", "helios-2.0", 4, gb(4));
    let piano = d.node("piano", "helios-2.0", 4, gb(4));

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
cargo test -- --test-threads 1
cargo test -- --test-threads 1 --ignored
```

Note that `cargo test` will automatically use `pfexec` to run tests; this is configured in
[.cargo/config.toml](.cargo/config.toml).

By default, topology and configuration for a falcon deployment is placed into
a hardcoded  `$PWD/.falcon` directory. However, users can override this by
setting the `Runner::falcon_dir` variable inside their code, and/or by passing
a `--falcon-dir <DIR>` parameter to most CLI commands. This allows tests and
code to be run independently as long as the names of the runners and nodes are
unique.

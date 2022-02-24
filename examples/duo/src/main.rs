// Copyright 2021 Oxide Computer Company

use libfalcon::{cli::run, error::Error, unit::gb, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("duo");

    // nodes, each with 2 cores and 2G of memory
    let violin = d.node("violin", "helios-1.0", 2, 2048);
    let piano = d.node("piano", "debian-11.0", 2, gb(2));

    // p9fs filesystem mounts
    // make sure you have a folder called "cargo-bay" in the working directory
    // where you execute falcon from
    d.mount("./cargo-bay", "/opt/stuff", violin)?;
    d.mount("./cargo-bay", "/opt/stuff", piano)?;

    // links
    d.link(violin, piano);

    run(&mut d).await?;
    Ok(())
}

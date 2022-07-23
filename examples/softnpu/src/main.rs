// Copyright 2021 Oxide Computer Company

use libfalcon::{cli::run, error::Error, unit::gb, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("duo");

    // nodes, each with 2 cores and 2G of memory
    let router = d.node("router", "helios-1.1", 2, gb(2));
    let violin = d.node("violin", "helios-1.1", 2, gb(2));
    let piano = d.node("piano", "helios-1.1", 2, gb(2));
    let cello = d.node("cello", "helios-1.1", 2, gb(2));

    // links
    d.softnpu_link(router, violin, None);
    d.softnpu_link(router, piano, None);
    d.softnpu_link(router, cello, None);

    run(&mut d).await?;
    Ok(())
}

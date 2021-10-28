// Copyright 2021 Oxide Computer Company

use libfalcon::{cli::{run, RunMode}, error::Error, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("duo");

    // nodes
    let violin = d.node("violin", "helios");
    let piano = d.node("piano", "helios");


    d.mount("/home/ry", "/opt/stuff", violin)?;
    d.mount("/home/ry", "/opt/stuff", piano)?;

    // links
    d.link(violin, piano);

    match run(&mut d).await? {
        RunMode::Launch => {
            d.exec(violin, "ipadm create-addr -t -T addrconf duo_violin_vnic0/v6")?;
            d.exec(piano,  "ipadm create-addr -t -T addrconf duo_violin_vnic0/v6")?;
            Ok(())
        }
        RunMode::Destroy => { Ok(()) }
    }
}

// Copyright 2021 Oxide Computer Company

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

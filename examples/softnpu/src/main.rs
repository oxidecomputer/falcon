// Copyright 2021 Oxide Computer Company

use libfalcon::{cli::{run, RunMode}, error::Error, unit::gb, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("duo");

    // nodes, each with 2 cores and 2G of memory
    let router = d.node("router", "netstack-1.2", 2, gb(2));
    let violin = d.node("violin", "helios-1.1", 2, gb(2));
    let piano = d.node("piano", "helios-1.1", 2, gb(2));
    let cello = d.node("cello", "helios-1.1", 2, gb(2));

    // links
    d.softnpu_link(
        router,
        violin,
        Some("a8:e1:de:00:00:01".into()),
        Some("a8:e1:de:01:70:1c".into())
    );
    d.softnpu_link(
        router,
        piano,
        Some("a8:e1:de:00:00:02".into()),
        Some("a8:e1:de:01:70:1d".into())
    );
    d.softnpu_link(
        router, 
        cello,
        Some("a8:e1:de:00:00:03".into()),
        Some("a8:e1:de:01:70:1e".into())
    );

    d.mount("./cargo-bay", "/opt/cargo-bay", router)?;
    d.mount("./cargo-bay", "/opt/cargo-bay", violin)?;
    d.mount("./cargo-bay", "/opt/cargo-bay", piano)?;
    d.mount("./cargo-bay", "/opt/cargo-bay", cello)?;

    match run(&mut d).await? {
        RunMode::Launch => {
            for node in [router, violin, piano, cello] {

                d.exec(node, &format!(
                    "chmod +x /opt/cargo-bay/{}-init.sh",
                    d.get_node(node).name,
                )).await?;

                d.exec(node, &format!(
                    "/opt/cargo-bay/{}-init.sh",
                    d.get_node(node).name,
                )).await?;

            }
        }
        _ => {}
    }
    Ok(())
}

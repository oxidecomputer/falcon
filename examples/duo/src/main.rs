// Copyright 2021 Oxide Computer Company

use libfalcon::{cli::{run, RunMode}, error::Error, Deployment};

fn main() -> Result<(), Error> {
    let mut d = Deployment::new("duo");

    // nodes
    let violin = d.node("violin", "helios");
    let piano = d.node("piano", "helios");

    d.mount("/home/ry", "/opt/stuff", violin)
        .expect("violin mount");
    d.mount("/home/ry", "/opt/stuff", piano)
        .expect("piano mount");

    // links
    d.link(violin, piano);

    match run(&mut d) {
        Ok(mode) => match mode {
            RunMode::Launch => { Ok(()) }
            RunMode::Destroy => { Ok(()) }
        },
        Err(e) => Err(e),
    }
}

// Copyright 2021 Oxide Computer Company

use libfalcon::{cli::run, Deployment};

fn main() {
    let mut d = Deployment::new("duo");

    // nodes
    let violin = d.zone("violin");
    let piano = d.zone("piano");

    d.mount("/home/ry", "/opt/stuff", violin)
        .expect("violin mount");
    d.mount("/home/ry", "/opt/stuff", piano)
        .expect("piano mount");

    // links
    d.link(violin, piano);

    run(&mut d);
}

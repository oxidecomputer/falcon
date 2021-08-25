// Copyright 2021 Oxide Computer Company

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

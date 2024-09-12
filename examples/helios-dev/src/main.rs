// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

use libfalcon::{cli::run, error::Error, unit::gb, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("hdev");

    // Modify cores or memory if you'd like
    let masaka = d.node("masaka", "helios-2.5", 4, gb(4));
    d.mount("./cargo-bay", "/opt/cargo-bay", masaka)?;

    // XXX Change this to point at your host machine's internet-facing network
    // interface.
    d.ext_link("igb0", masaka);

    run(&mut d).await?;

    Ok(())
}

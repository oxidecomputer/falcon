// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

use libfalcon::{cli::run, error::Error, unit::gb, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("solo");

    d.node("violin", "netstack-1.5", 2, gb(2));
    //d.node("violin", "helios-1.1", 2, gb(2));
    run(&mut d).await?;
    Ok(())
}

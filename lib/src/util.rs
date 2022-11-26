// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

pub(crate) static NAME_REGEX: &str = r"^[A-Za-z]?[A-Za-z0-9_]*$";

#[macro_export]
macro_rules! die {
    ($x:expr, $($xs:expr),*) => {
        println!($x,$($xs),*);
        std::process::exit(1);
    };

    ($x:expr) => {
        use std::process;
        println!($x);
        std::process::exit(1);
    };
}

#[macro_export]
macro_rules! namecheck {
    ($x:expr, $what:expr) => {
        let re = regex::Regex::new($crate::util::NAME_REGEX)
            .expect("name regex compilation failed");

        if !re.is_match($x) {
            die!("{} name must match {}", $what, $crate::util::NAME_REGEX);
        }
    };
}

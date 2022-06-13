// Copyright 2021 Oxide Computer Company

pub(crate) static NAME_REGEX: &str = r"[A-Za-z]?[A-Za-z0-9_]*";

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
        let re = regex::Regex::new(crate::util::NAME_REGEX)
            .expect("name regex compilation failed");

        if !re.is_match($x) {
            die!("{} name must match {}", $what, crate::util::NAME_REGEX);
        }
    };
}

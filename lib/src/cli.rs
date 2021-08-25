// Copyright 2021 Oxide Computer Company

use crate::Deployment;
use std::{env, process};

pub enum RunMode {
    Launch,
    Destroy,
    Error,
}

/// Entry point for a command line application. Will parse command line
/// arguments and take actions accordingly.
///
/// # Examples
/// ```no_run
/// use libfalcon::{cli::run, Deployment};
/// fn main() {
///     let mut d = Deployment::new("duo");
///
///     // nodes
///     let violin = d.zone("violin");
///     let piano = d.zone("piano");
///
///     // links
///     d.link(violin, piano);
///
///     run(&mut d);
/// }
/// ```
pub fn run(d: &mut Deployment) -> RunMode {
    d.persistent = true;

    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        usage(&args);
    }

    match args[1].as_str() {
        "launch" => {
            launch(d);
            RunMode::Launch
        }
        "destroy" => {
            destroy(d);
            RunMode::Destroy
        }
        _ => {
            usage(&args);
            RunMode::Error
        }
    }
}

fn launch(d: &Deployment) {
    match d.launch() {
        Err(e) => println!("{}", e),
        Ok(()) => {}
    }
}

fn destroy(d: &Deployment) {
    match d.destroy() {
        Err(e) => println!("{}", e),
        Ok(()) => {}
    }
}

fn usage(args: &Vec<String>) {
    println!("usage: {} (launch | destroy)", args[0]);
    process::exit(1);
}

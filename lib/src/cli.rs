// Copyright 2021 Oxide Computer Company

use crate::{error::Error, Runner};
use std::env;

pub enum RunMode {
    Launch,
    Destroy,
}

/// Entry point for a command line application. Will parse command line
/// arguments and take actions accordingly.
///
/// # Examples
/// ```no_run
/// use libfalcon::{cli::run, Runner};
/// fn main() {
///     let mut r = Runner::new("duo");
///
///     // nodes
///     let violin = r.zone("violin");
///     let piano = r.zone("piano");
///
///     // links
///     r.link(violin, piano);
///
///     run(&mut r);
/// }
/// ```
pub async fn run(r: &mut Runner) -> Result<RunMode, Error> {
    r.persistent = true;

    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        return Err(Error::Cli(usage(&args)));
    }

    match args[1].as_str() {
        "launch" => {
            launch(r).await;
            Ok(RunMode::Launch)
        }
        "destroy" => {
            destroy(r);
            Ok(RunMode::Destroy)
        }
        _ => {
            Err(Error::Cli(usage(&args)))
        }
    }
}

async fn launch(r: &Runner) {
    match r.launch().await {
        Err(e) => println!("{}", e),
        Ok(()) => {}
    }
}

fn destroy(r: &Runner) {
    match r.destroy() {
        Err(e) => println!("{}", e),
        Ok(()) => {}
    }
}

fn usage(args: &Vec<String>) -> String {
    format!("usage: {} (launch | destroy)", args[0])
}

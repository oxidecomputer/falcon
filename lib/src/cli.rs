// Copyright 2021 Oxide Computer Company

use std::fs;
use std::{
    net::{IpAddr, SocketAddr, Ipv4Addr},
    os::unix::prelude::AsRawFd,
    io::{stdout, Write},
};

use anyhow::{anyhow, Context};
use futures::{SinkExt, StreamExt};
use propolis_client::{
    api::InstanceStateRequested,
    Client,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
use slog::{o, Drain, Level, Logger};
use colored::*;
use tabwriter::TabWriter;

use clap::{AppSettings, Parser};

use crate::{error::Error, Runner};

pub enum RunMode {
    Unspec,
    Launch,
    Destroy,
}

#[derive(Parser)]
#[clap(
    version = "0.1",
    author = "Ryan Goodfellow <ryan.goodfellow@oxide.computer>"
)]
#[clap(setting = AppSettings::InferSubcommands)]
struct Opts {
    #[clap(short, long, parse(from_occurrences))]
    verbose: i32,

    #[clap(subcommand)]
    subcmd: SubCommand,
}


#[derive(Parser)]
enum SubCommand {
    #[clap(about = "launch topology")]
    Launch(CmdLaunch),
    #[clap(about = "destroy topology")]
    Destroy(CmdDestroy),
    #[clap(about = "get a serial console session for the specified vm")]
    Serial(CmdSerial),
    #[clap(about = "display topology information")]
    Info(CmdInfo),
    #[clap(about = "reboot a vm")]
    Reboot(CmdReboot),
}

#[derive(Parser)]
#[clap(setting = AppSettings::InferSubcommands)]
struct CmdLaunch {}

#[derive(Parser)]
#[clap(setting = AppSettings::InferSubcommands)]
struct CmdDestroy {}

#[derive(Parser)]
#[clap(setting = AppSettings::InferSubcommands)]
struct CmdSerial {
    vm_name: String,
}

#[derive(Parser)]
#[clap(setting = AppSettings::InferSubcommands)]
struct CmdReboot {
    vm_name: String,
}

#[derive(Parser)]
#[clap(setting = AppSettings::InferSubcommands)]
struct CmdInfo {}

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

    let opts: Opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Launch(_) => {
            launch(r).await;
            Ok(RunMode::Launch)
        },
        SubCommand::Destroy(_) => {
            destroy(r);
            Ok(RunMode::Destroy)
        },
        SubCommand::Serial(ref c) => {
            console(&c.vm_name).await?;
            Ok(RunMode::Unspec)
        },
        SubCommand::Info(_) => {
            info(r)?;
            Ok(RunMode::Unspec)
        }
        SubCommand::Reboot(ref c) => {
            reboot(&c.vm_name).await?;
            Ok(RunMode::Unspec)
        },
    }

}

fn info(r: &Runner) -> anyhow::Result<()> {

    let mut tw = TabWriter::new(stdout());

    println!("{} {}",
        "name:".dimmed(),
        r.deployment.name,
    );

    println!("{}", "Nodes".bright_black());
    write!(
        &mut tw,
        "{}\t{}\t{}\t{}\t{}\n",
        "Name".dimmed(),
        "Image".dimmed(),
        "Radix".dimmed(),
        "Mounts".dimmed(),
        "UUID".dimmed(),
    )?;
    write!(
        &mut tw,
        "{}\t{}\t{}\t{}\t{}\n",
        "----".bright_black(),
        "-----".bright_black(),
        "-----".bright_black(),
        "------".bright_black(),
        "----".bright_black(),
    )?;
    for x in &r.deployment.nodes {
        let mount = {
            if x.mounts.len() > 0 {
                format!("{} -> {}",
                    x.mounts[0].source,
                    x.mounts[0].destination,
                )
            } else {
                "".into()
            }
        };
        write!(
            &mut tw,
            "{}\t{}\t{}\t{}\t{}\n",
            x.name,
            x.image,
            x.radix,
            mount,
            x.id,
        )?;
        if x.mounts.len() > 1 {
            for m in &x.mounts[1..] {
                let mount = format!("{} -> {}",
                    m.source,
                    m.destination,
                );
                write!(
                    &mut tw,
                    "{}\t{}\t{}\t{}\t{}\n",
                    "",
                    "",
                    "",
                    mount,
                    "",
                )?;
            }
        }
    }
    tw.flush()?;

    Ok(())

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

async fn console(name: &str) -> Result<(), Error> {

    let port: u16 =
        fs::read_to_string(format!(".falcon/{}.port", name))?.parse()?;

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127,0,0,1)), port);
    let log = create_logger();
    let client = Client::new(addr.clone(), log.new(o!()));

    serial(
        &client,
        addr.clone(),
        name.into(),
    ).await?;

    Ok(())

}

// TODO copy pasta from propolis/cli/src/main.rs
async fn serial(
    client: &Client,
    addr: SocketAddr,
    name: String,
) -> anyhow::Result<()> {
    // Grab the Instance UUID
    let id = client
        .instance_get_uuid(&name)
        .await
        .with_context(|| anyhow!("failed to get instance UUID"))?;

    let path = format!("ws://{}/instances/{}/serial", addr, id);
    let (mut ws, _) = tokio_tungstenite::connect_async(path)
        .await
        .with_context(|| anyhow!("failed to create serial websocket stream"))?;

    let _raw_guard = RawTermiosGuard::stdio_guard()
        .with_context(|| anyhow!("failed to set raw mode"))?;

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    loop {
        tokio::select! {
            c = stdin.read_u8() => {
                match c? {
                    // Exit on Ctrl-Q
                    b'\x11' => break,
                    c => ws.send(Message::binary(vec![c])).await?,
                }
            }
            msg = ws.next() => {
                match msg {
                    Some(Ok(Message::Binary(input))) => {
                        stdout.write_all(&input).await?;
                        stdout.flush().await?;
                    }
                    Some(Ok(Message::Close(..))) | None => break,
                    _ => continue,
                }
            }
        }
    }

    Ok(())
}

/// Guard object that will set the terminal to raw mode and restore it
/// to its previous state when it's dropped
struct RawTermiosGuard(libc::c_int, libc::termios);

impl RawTermiosGuard {
    fn stdio_guard() -> Result<RawTermiosGuard, std::io::Error> {
        let fd = std::io::stdout().as_raw_fd();
        let termios = unsafe {
            let mut curr_termios = std::mem::zeroed();
            let r = libc::tcgetattr(fd, &mut curr_termios);
            if r == -1 {
                return Err(std::io::Error::last_os_error());
            }
            curr_termios
        };
        let guard = RawTermiosGuard(fd, termios.clone());
        unsafe {
            let mut raw_termios = termios;
            libc::cfmakeraw(&mut raw_termios);
            let r = libc::tcsetattr(fd, libc::TCSAFLUSH, &raw_termios);
            if r == -1 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(guard)
    }
}
impl Drop for RawTermiosGuard {
    fn drop(&mut self) {
        let r = unsafe { libc::tcsetattr(self.0, libc::TCSADRAIN, &self.1) };
        if r == -1 {
            Err::<(), _>(std::io::Error::last_os_error()).unwrap();
        }
    }
}

/// Create a top-level logger that outputs to stderr
fn create_logger() -> Logger {
    let decorator = slog_term::TermDecorator::new().stderr().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let level =  Level::Debug;
    let drain = slog::LevelFilter(drain, level).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    let logger = Logger::root(drain, o!());
    logger
}

async fn reboot(name: &str) -> Result<(), Error> {

    let port: u16 =
        fs::read_to_string(format!(".falcon/{}.port", name))?.parse()?;

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127,0,0,1)), port);
    let log = create_logger();
    let client = Client::new(addr.clone(), log.new(o!()));

    // Grab the Instance UUID
    let id = client
        .instance_get_uuid(&name)
        .await
        .with_context(|| anyhow!("failed to get instance UUID"))?;

    // reboot
    client
        .instance_state_put(id, InstanceStateRequested::Reboot)
        .await
        .with_context(|| anyhow!("failed to reboot machine"))?;

    Ok(())

}


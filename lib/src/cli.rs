// Copyright 2021 Oxide Computer Company

use std::env;
use std::fs;

use crate::{error::Error, Runner};





use std::{
    net::{IpAddr, SocketAddr, Ipv4Addr},
    os::unix::prelude::AsRawFd,
};

use anyhow::{anyhow, Context};
use futures::{SinkExt, StreamExt};
use propolis_client::Client;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
use slog::{o, Drain, Level, Logger};





pub enum RunMode {
    Launch,
    Destroy,
    Console,
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

    if args.len() < 2 {
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
        "console" => {
            if args.len() < 3 {
                return Err(Error::Cli(
                        "must provide node name argument".into()));
            }
            console(args[2].as_str()).await?;
            Ok(RunMode::Console)
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

fn usage(args: &Vec<String>) -> String {
    format!("usage: {} (launch | destroy | console)", args[0])
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

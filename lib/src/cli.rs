// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

use std::fs;
use std::process::Command;
use std::{
    future::Future,
    io::{stdout, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::prelude::AsRawFd,
};

use anyhow::{anyhow, Context};
use camino::{Utf8Path, Utf8PathBuf};
use clap::ArgAction;
use colored::*;
use futures::{SinkExt, StreamExt};
use propolis_client::{types::InstanceStateRequested, Client};
use ron::de::from_str;
use slog::{o, warn, Drain, Level, Logger};
use tabwriter::TabWriter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

use clap::Parser;

use crate::{dataset, error::Error, Deployment, Runner, DEFAULT_FALCON_DIR};

pub enum RunMode {
    Unspec,
    Launch,
    Destroy,
}

#[derive(Parser)]
#[clap(version = "0.1")]
#[clap(infer_subcommands = true, styles = oxide_cli_style())]
pub struct Opts<T: clap::Args> {
    #[clap(short, long, action = ArgAction::Count)]
    verbose: u8,

    #[clap(subcommand)]
    subcmd: SubCommand<T>,
}

#[derive(Parser)]
enum SubCommand<T: clap::Args> {
    #[clap(about = "run topology preflight")]
    Preflight(CmdPreflight),
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
    #[clap(about = "stop a vm's hypervisor")]
    Hyperstop(CmdHyperstop),
    #[clap(about = "start a vm's hypervisor")]
    Hyperstart(CmdHyperstart),
    #[clap(about = "create a topology's network")]
    Netcreate(CmdNetCreate),
    #[clap(about = "destroy a topology's network")]
    Netdestroy(CmdNetDestroy),
    #[clap(about = "snapshot a node")]
    Snapshot(CmdSnapshot),
    #[clap(about = "execute a command on a node")]
    Exec(CmdExec),
    /// Allow users of this library to add their own set of commands
    #[clap(about = "topology-specific commands")]
    Extra(T),
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdLaunch {
    /// The propolis-server binary to use
    #[clap(short, long)]
    propolis: Option<String>,

    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdPreflight {
    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdDestroy {
    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdSerial {
    /// Name of the VM to establish a serial connection to
    vm_name: String,

    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdReboot {
    /// Name of the VM to reboot
    vm_name: String,

    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdHyperstop {
    /// Name of the vm to stop
    vm_name: Option<String>,

    /// Stop all vms in the topology
    #[clap(short, long)]
    all: bool,

    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdHyperstart {
    /// The propolis-server binary to use
    #[clap(short, long)]
    propolis: Option<String>,

    /// Name of the vm to start
    vm_name: Option<String>,

    /// Start all vms in the topology
    #[clap(short, long)]
    all: bool,

    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdNetCreate {}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdNetDestroy {}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdSnapshot {
    /// Name of the VM to snaphost
    vm_name: String,

    /// What to name the new snapshot
    snapshot_name: String,

    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdInfo {}

#[derive(Parser)]
#[clap(infer_subcommands = true)]
struct CmdExec {
    node: String,
    command: String,

    /// The path of the falcon output directory
    #[clap(short, long, default_value_t = Utf8PathBuf::from(DEFAULT_FALCON_DIR))]
    falcon_dir: Utf8PathBuf,
}

/// Entry point for a command line application. Will parse command line
/// arguments and take actions accordingly.
///
/// # Examples
/// ```no_run
/// use libfalcon::{cli::run, Runner};
///
/// let mut r = Runner::new("duo");
///
/// // nodes
/// let violin = r.node("violin", "helios-2.5", 1, 1024);
/// let piano = r.node("piano", "helios-2.5", 1, 1024);
///
/// // links
/// r.link(violin, piano);
///
/// run(&mut r);
/// ```
pub async fn run(r: &mut Runner) -> Result<RunMode, Error> {
    #[derive(Parser)]
    struct Dummy {}
    run_with_extra(r, |_: &mut Runner, _: Dummy| std::future::ready(Ok(())))
        .await
}

/// Entry point for a command line application. Will parse command line
/// arguments (including any extra commands that users of this library add) and
/// take actions accordingly.
///
/// # Examples
/// ```no_run
/// use libfalcon::{
///     cli::run_with_extra,
///     Runner,
///     error::Error,
/// };
/// use clap::Parser;
///
/// #[derive(Parser)]
/// #[command(version = "0.1")]
/// struct OtherOpts {
///     #[clap(subcommand)]
///     subcmd: SubCommand,
/// }
///
/// #[derive(Parser)]
/// enum SubCommand {
///     #[clap(about = "some topology specific command")]
///     Something,
/// }
///
/// async fn extra_stuff(r: &mut Runner, opts: OtherOpts) -> Result<(), Error> {
///     match opts.subcmd {
///         SubCommand::Something => {
///             // some topology specific command!
///         }
///     }
///
///     Ok(())
/// }
///
/// let mut r = Runner::new("duo");
///
/// // nodes
/// let violin = r.node("violin", "helios-2.5", 1, 1024);
/// let piano = r.node("piano", "helios-2.5", 1, 1024);
///
/// // links
/// r.link(violin, piano);
///
/// run_with_extra(&mut r, extra_stuff);
/// ```
pub async fn run_with_extra<'a, T, F, O>(
    r: &'a mut Runner,
    extra: F,
) -> Result<RunMode, Error>
where
    T: clap::Args,
    F: FnOnce(&'a mut Runner, T) -> O,
    O: Future<Output = Result<(), Error>>,
{
    r.persistent = true;

    let opts: Opts<T> = Opts::<T>::parse();
    match opts.subcmd {
        SubCommand::Preflight(p) => {
            r.set_falcon_dir(Some(p.falcon_dir.to_string()));
            preflight(r).await;
            Ok(RunMode::Unspec)
        }
        SubCommand::Launch(l) => {
            if let Some(path) = l.propolis {
                r.set_propolis_binary(Some(path));
            }
            r.set_falcon_dir(Some(l.falcon_dir.to_string()));
            launch(r).await;
            Ok(RunMode::Launch)
        }
        SubCommand::Destroy(d) => {
            r.set_falcon_dir(Some(d.falcon_dir.to_string()));
            destroy(r);
            Ok(RunMode::Destroy)
        }
        SubCommand::Serial(ref c) => {
            console(&c.vm_name, &c.falcon_dir).await?;
            Ok(RunMode::Unspec)
        }
        SubCommand::Info(_) => {
            info(r)?;
            Ok(RunMode::Unspec)
        }
        SubCommand::Reboot(ref c) => {
            reboot(&c.vm_name, &c.falcon_dir).await?;
            Ok(RunMode::Unspec)
        }
        SubCommand::Hyperstop(ref c) => {
            if c.all {
                for x in &r.deployment.nodes {
                    hyperstop(&x.name, &c.falcon_dir).await?;
                }
            } else {
                match c.vm_name {
                    None => {
                        return Err(Error::Cli(
                            "vm name required unless --all flag is used".into(),
                        ))
                    }
                    Some(ref n) => hyperstop(n, &c.falcon_dir).await?,
                }
            }
            Ok(RunMode::Unspec)
        }
        SubCommand::Hyperstart(ref c) => {
            let propolis_binary = match c.propolis {
                Some(ref path) => path.clone(),
                None => "propolis-server".into(),
            };
            if c.all {
                for x in &r.deployment.nodes {
                    hyperstart(
                        &x.name,
                        propolis_binary.clone(),
                        c.falcon_dir.clone().into(),
                    )
                    .await?;
                }
            } else {
                match c.vm_name {
                    None => {
                        return Err(Error::Cli(
                            "vm name required unless --all flag is used".into(),
                        ))
                    }
                    Some(ref n) => {
                        hyperstart(
                            n,
                            propolis_binary,
                            c.falcon_dir.clone().into(),
                        )
                        .await?
                    }
                }
            }
            Ok(RunMode::Unspec)
        }
        SubCommand::Netcreate(_) => {
            netcreate(r).await;
            Ok(RunMode::Unspec)
        }
        SubCommand::Netdestroy(_) => {
            netdestroy(r);
            Ok(RunMode::Unspec)
        }
        SubCommand::Snapshot(s) => {
            snapshot(s)?;
            Ok(RunMode::Unspec)
        }
        SubCommand::Exec(ref c) => {
            r.set_falcon_dir(Some(c.falcon_dir.clone().into()));
            exec(r, &c.node, &c.command).await?;
            Ok(RunMode::Unspec)
        }
        SubCommand::Extra(c) => {
            extra(r, c).await?;
            Ok(RunMode::Unspec)
        }
    }
}

fn info(r: &Runner) -> anyhow::Result<()> {
    let mut tw = TabWriter::new(stdout());

    println!("{} {}", "name:".dimmed(), r.deployment.name,);

    println!("{}", "Nodes".bright_black());
    writeln!(
        &mut tw,
        "{}\t{}\t{}\t{}\t{}",
        "Name".dimmed(),
        "Image".dimmed(),
        "Radix".dimmed(),
        "Mounts".dimmed(),
        "UUID".dimmed(),
    )?;
    writeln!(
        &mut tw,
        "{}\t{}\t{}\t{}\t{}",
        "----".bright_black(),
        "-----".bright_black(),
        "-----".bright_black(),
        "------".bright_black(),
        "----".bright_black(),
    )?;
    for x in &r.deployment.nodes {
        let mount = {
            if !x.mounts.is_empty() {
                format!("{} -> {}", x.mounts[0].source, x.mounts[0].destination,)
            } else {
                "".into()
            }
        };
        writeln!(
            &mut tw,
            "{}\t{}\t{}\t{}\t{}",
            x.name, x.image, x.radix, mount, x.id,
        )?;
        if x.mounts.len() > 1 {
            for m in &x.mounts[1..] {
                let mount = format!("{} -> {}", m.source, m.destination,);
                writeln!(&mut tw, "\t\t\t{}\t", mount)?;
            }
        }
    }
    tw.flush()?;

    Ok(())
}

async fn preflight(r: &mut Runner) {
    if let Err(e) = r.preflight().await {
        eprintln!("error: {}", e)
    }
}

async fn launch(r: &mut Runner) {
    if let Err(e) = r.launch().await {
        eprintln!("error: {}", e)
    }
}

async fn netcreate(r: &Runner) {
    if let Err(e) = r.net_launch().await {
        eprintln!("error: {}", e)
    }
}

fn netdestroy(r: &Runner) {
    if let Err(e) = r.net_destroy() {
        eprintln!("error: {}", e)
    }
}

fn snapshot(cmd: CmdSnapshot) -> Result<(), Error> {
    // read topology
    let mut path = cmd.falcon_dir.to_path_buf();
    path.push("topology.ron");
    let topo_ron = fs::read_to_string(&path)?;
    let d: Deployment = from_str(&topo_ron)?;
    path.pop();

    // get node from topology
    let mut node = None;
    for n in &d.nodes {
        if n.name == cmd.vm_name {
            node = Some(n);
        }
    }

    let node = match node {
        None => return Err(Error::NotFound(cmd.vm_name)),
        Some(node) => node,
    };

    let dataset = dataset();

    let source = format!("{}/topo/{}/{}", dataset, d.name, node.name);
    let source_snapshot = format!("{}@base", source);

    let dest = format!("{}/img/{}", dataset, cmd.snapshot_name,);
    let dest_snapshot = format!("{}@base", source);

    // first take a snapshot of the node clone
    let out = Command::new("zfs")
        .args(["snapshot", source_snapshot.as_ref()])
        .output()?;
    if !out.status.success() {
        return Err(Error::Zfs(String::from_utf8(out.stderr)?));
    }

    // next clone the source snapshot to a new base image
    let out = Command::new("zfs")
        .args(["clone", source_snapshot.as_ref(), dest.as_ref()])
        .output()?;

    if !out.status.success() {
        return Err(Error::Zfs(String::from_utf8(out.stderr)?));
    }

    // promote the base image to uncouple from source snapshot
    let out = Command::new("zfs")
        .args(["promote", dest.as_ref()])
        .output()?;
    if !out.status.success() {
        return Err(Error::Zfs(String::from_utf8(out.stderr)?));
    }

    // finally create base snapshot for new image
    let out = Command::new("zfs")
        .args(["snapshot", dest_snapshot.as_ref()])
        .output()?;
    if !out.status.success() {
        return Err(Error::Zfs(String::from_utf8(out.stderr)?));
    }

    Ok(())
}

fn destroy(r: &Runner) {
    if let Err(e) = r.destroy() {
        println!("{}", e)
    }
}

async fn console(name: &str, falcon_dir: &Utf8Path) -> Result<(), Error> {
    println!(
        "{}\n{}\n{}",
        "Entering VM console.".blue(),
        "Use ^q to exit.".bright_blue(),
        "Press enter to continue.".bright_blue()
    );
    let mut path = falcon_dir.to_path_buf();
    path.push(format!("{name}.port"));
    let port: u16 = fs::read_to_string(&path)?.trim_end().parse()?;
    path.pop();

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);
    serial(addr).await?;

    Ok(())
}

// TODO copy pasta from propolis/cli/src/main.rs
async fn serial(addr: SocketAddr) -> anyhow::Result<()> {
    let path = format!("ws://{}/instance/serial", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(path)
        .await
        .with_context(|| anyhow!("failed to create serial websocket stream"))?;

    let _raw_guard = RawTermiosGuard::stdio_guard()
        .with_context(|| anyhow!("failed to set raw mode"))?;

    let mut stdout = tokio::io::stdout();

    // https://docs.rs/tokio/latest/tokio/io/trait.AsyncReadExt.html#method.read_exact
    // is not cancel safe! Meaning reads from tokio::io::stdin are not cancel
    // safe. Spawn a separate task to read and put bytes onto this channel.
    let (stdintx, stdinrx) = tokio::sync::mpsc::channel(16);
    let (wstx, mut wsrx) = tokio::sync::mpsc::channel(16);

    tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let mut inbuf = [0u8; 1024];

        loop {
            let n = match stdin.read(&mut inbuf).await {
                Err(_) | Ok(0) => break,
                Ok(n) => n,
            };

            stdintx.send(inbuf[0..n].to_vec()).await.unwrap();
        }
    });

    tokio::spawn(async move { stdin_to_websockets_task(stdinrx, wstx).await });

    loop {
        tokio::select! {
            c = wsrx.recv() => {
                match c {
                    None => {
                        // channel is closed
                        break;
                    }
                    Some(c) => {
                        ws.send(Message::Binary(c)).await?;
                    },
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

// TODO copy pasta from propolis/cli/src/main.rs
async fn stdin_to_websockets_task(
    mut stdinrx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    wstx: tokio::sync::mpsc::Sender<Vec<u8>>,
) {
    loop {
        let inbuf = if let Some(inbuf) = stdinrx.recv().await {
            inbuf
        } else {
            continue;
        };

        // Put bytes from inbuf to outbuf,
        let mut outbuf = Vec::with_capacity(inbuf.len());

        let mut exit = false;
        for c in inbuf {
            match c {
                b'\x11' => {
                    // Exit onCtrl-Q
                    exit = true;
                    break;
                }
                _ => {
                    outbuf.push(c);
                }
            }
        }

        // Send what we have, even if there's a Ctrl-C at the end.
        if !outbuf.is_empty() {
            wstx.send(outbuf).await.unwrap();
        }

        if exit {
            break;
        }
    }
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
        let guard = RawTermiosGuard(fd, termios);
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
            panic!("{:?}", std::io::Error::last_os_error());
        }
    }
}

/// Create a top-level logger that outputs to stderr
fn create_logger() -> Logger {
    let decorator = slog_term::TermDecorator::new().stderr().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let level = Level::Debug;
    let drain = slog::LevelFilter(drain, level).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    Logger::root(drain, o!())
}

async fn reboot(name: &str, falcon_dir: &Utf8Path) -> Result<(), Error> {
    let mut path = falcon_dir.to_path_buf();
    path.push(format!("{name}.port"));
    let port: u16 = fs::read_to_string(&path)?.trim_end().parse()?;
    path.pop();

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port);
    let client = Client::new(&format!("http://{}", addr));

    // reboot
    client
        .instance_state_put()
        .body(InstanceStateRequested::Reboot)
        .send()
        .await
        .with_context(|| anyhow!("failed to reboot machine"))?;

    Ok(())
}

async fn hyperstop(name: &str, falcon_dir: &Utf8Path) -> Result<(), Error> {
    let log = create_logger();

    let mut path = falcon_dir.to_path_buf();
    path.push(format!("{name}.pid"));

    // read pid
    match fs::read_to_string(&path) {
        Ok(pid) => match pid.trim_end().parse() {
            Ok(pid) => {
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
                fs::remove_file(&path)?;
            }
            Err(e) => warn!(log, "could not parse pidfile for {}: {}", name, e),
        },
        Err(e) => {
            warn!(log, "could not get pidfile for {}: {}", name, e);
        }
    };
    path.pop();

    // get instance uuid
    path.push(format!("{name}.uuid"));
    let uuid = match fs::read_to_string(&path) {
        Ok(u) => u,
        Err(e) => {
            warn!(log, "get propolis uuid for {}: {}", name, e);
            return Ok(());
        }
    };
    path.pop();

    // destroy bhyve vm
    let vm_arg = format!("--vm={}", uuid);
    match Command::new("bhyvectl")
        .args(["--destroy", vm_arg.as_ref()])
        .output()
    {
        Ok(_) => {}
        Err(e) => {
            warn!(log, "delete bhyve vm for {}: {}", name, e);
            return Ok(());
        }
    }

    Ok(())
}

async fn hyperstart(
    name: &str,
    propolis_binary: String,
    falcon_dir: String,
) -> Result<(), Error> {
    // read topology
    let mut path: Utf8PathBuf = falcon_dir.clone().into();
    path.push("topology.ron");
    let topo_ron = fs::read_to_string(&path)?;
    let d: Deployment = from_str(&topo_ron)?;
    path.pop();

    let mut node = None;
    for n in &d.nodes {
        if n.name == name {
            node = Some(n);
        }
    }

    let node = match node {
        None => return Err(Error::NotFound(name.into())),
        Some(node) => node,
    };

    path.push(format!("{name}.uuid"));
    let id: uuid::Uuid = fs::read_to_string(&path)?.trim_end().parse()?;
    path.pop();
    let log = create_logger();

    crate::launch_vm(&log, &propolis_binary, &id, node, &falcon_dir, None)
        .await?;

    Ok(())
}

async fn exec(r: &Runner, node: &str, command: &str) -> Result<(), Error> {
    println!("{}", r.do_exec(node, command).await?);
    Ok(())
}

pub fn oxide_cli_style() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .header(anstyle::Style::new().bold().underline().fg_color(Some(
            anstyle::Color::Rgb(anstyle::RgbColor(245, 207, 101)),
        )))
        .literal(anstyle::Style::new().bold().fg_color(Some(
            anstyle::Color::Rgb(anstyle::RgbColor(72, 213, 151)),
        )))
        .invalid(anstyle::Style::new().bold().fg_color(Some(
            anstyle::Color::Rgb(anstyle::RgbColor(72, 213, 151)),
        )))
        .valid(anstyle::Style::new().bold().fg_color(Some(
            anstyle::Color::Rgb(anstyle::RgbColor(72, 213, 151)),
        )))
        .usage(anstyle::Style::new().bold().fg_color(Some(
            anstyle::Color::Rgb(anstyle::RgbColor(245, 207, 101)),
        )))
        .error(anstyle::Style::new().bold().fg_color(Some(
            anstyle::Color::Rgb(anstyle::RgbColor(232, 104, 134)),
        )))
}

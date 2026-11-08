// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2026 Oxide Computer Company

use clap::{Args, Parser, Subcommand};
use falcon_img::{
    Config, DEFAULT_ASSETS_UPSTREAM, DEFAULT_DIR, DEFAULT_PORT,
    DEFAULT_PROPOLIS_UPSTREAM,
};
use slog::Drain;
use std::net::{Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use tokio::net::TcpListener;

/// Caching image server for falcon topologies.
///
/// Falcon looks for this server at the DNS name `falcon-img` on port 7780.
#[derive(Parser)]
#[command(version)]
struct Opts {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the image server.
    Serve(ServeArgs),
    /// Print the cache directory. Images placed in this directory are served
    /// as-is, e.g. `mv helios-2.5_0.raw.xz $(falcon-img dir)`.
    Dir(DirArg),
}

#[derive(Args)]
struct DirArg {
    /// Cache directory.
    #[arg(long, env = "FALCON_IMG_DIR", default_value = DEFAULT_DIR)]
    dir: PathBuf,
}

#[derive(Args)]
struct ServeArgs {
    #[command(flatten)]
    dir: DirArg,

    /// Address to listen on.
    #[arg(
        long,
        env = "FALCON_IMG_LISTEN",
        default_value_t = SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), DEFAULT_PORT),
    )]
    listen: SocketAddr,

    /// Never contact upstream; only serve what is already in the cache. For
    /// air-gapped environments.
    #[arg(
        long,
        env = "FALCON_IMG_OFFLINE",
        num_args = 0..=1,
        require_equals = true,
        default_value_t = false,
        default_missing_value = "true",
        value_parser = clap::builder::BoolishValueParser::new(),
    )]
    offline: bool,

    /// Upstream base URL for falcon assets (images and OVMF).
    #[arg(
        long,
        env = "FALCON_IMG_ASSETS_UPSTREAM",
        default_value = DEFAULT_ASSETS_UPSTREAM,
    )]
    assets_upstream: String,

    /// Upstream base URL for propolis-server builds.
    #[arg(
        long,
        env = "FALCON_IMG_PROPOLIS_UPSTREAM",
        default_value = DEFAULT_PROPOLIS_UPSTREAM,
    )]
    propolis_upstream: String,
}

fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();
    match opts.cmd {
        Cmd::Dir(d) => {
            let dir = std::path::absolute(&d.dir).unwrap_or(d.dir);
            println!("{}", dir.display());
            Ok(())
        }
        Cmd::Serve(s) => {
            let log = logger();
            let rt = tokio::runtime::Runtime::new()?;
            let config = Config {
                dir: s.dir.dir,
                assets_upstream: s
                    .assets_upstream
                    .trim_end_matches('/')
                    .to_owned(),
                propolis_upstream: s
                    .propolis_upstream
                    .trim_end_matches('/')
                    .to_owned(),
                offline: s.offline,
            };
            rt.block_on(async {
                let listener = TcpListener::bind(s.listen).await?;
                falcon_img::serve(listener, config, log).await
            })
        }
    }
}

fn logger() -> slog::Logger {
    if std::env::var("RUST_LOG")
        .map(|s| s.is_empty())
        .unwrap_or(true)
    {
        // SAFETY: called before any other threads are started.
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }
    let decorator = slog_term::PlainDecorator::new(std::io::stderr());
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_envlogger::new(drain).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();
    slog::Logger::root(drain, slog::o!())
}

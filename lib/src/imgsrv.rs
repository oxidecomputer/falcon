// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2026 Oxide Computer Company

//! Discovery of a falcon image server (`falcon-img`), and the resulting
//! choice of where to download images, OVMF and propolis-server from.

use anyhow::{anyhow, Context};
use serde::Deserialize;
use slog::{info, Logger};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::time::Duration;

/// The name falcon resolves to find an image server when none is specified.
pub const DEFAULT_IMG_SERVER: &str = "falcon-img";

/// The port used when an image server is given without one.
pub const DEFAULT_IMG_SERVER_PORT: u16 = 7780;

/// Environment variable naming the image server (`host` or `host:port`).
pub const IMG_SERVER_ENV: &str = "FALCON_IMG_SERVER";

/// Environment variable that disables image server discovery when set to a
/// truthy value.
pub const NO_IMG_DISCOVERY_ENV: &str = "FALCON_NO_IMG_DISCOVERY";

const FALCON_ASSETS_URL: &str =
    "https://oxide-falcon-assets.s3.us-west-2.amazonaws.com";
const PROPOLIS_URL: &str =
    "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon";

/// Service name and API version expected from the discovery endpoint. These
/// must match the `falcon-img` crate.
const SERVICE_NAME: &str = "falcon-img";
const API_VERSION: u32 = 1;

/// How falcon should look for an image server.
#[derive(Clone, Debug)]
pub struct ImgServerConfig {
    /// An explicitly requested image server. When set, failing to reach it is
    /// an error. When unset, [`DEFAULT_IMG_SERVER`] is tried and its absence
    /// from DNS means falcon downloads from upstream sources.
    pub server: Option<String>,
    /// Whether to look for an image server at all.
    pub discovery: bool,
}

impl Default for ImgServerConfig {
    fn default() -> Self {
        Self {
            server: None,
            discovery: true,
        }
    }
}

impl ImgServerConfig {
    /// Build a config from [`IMG_SERVER_ENV`] and [`NO_IMG_DISCOVERY_ENV`].
    pub fn from_env() -> Self {
        let server = std::env::var(IMG_SERVER_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty());
        let disabled = std::env::var(NO_IMG_DISCOVERY_ENV)
            .map(|v| truthy(&v))
            .unwrap_or(false);
        Self {
            server,
            discovery: !disabled,
        }
    }
}

/// Matches clap's interpretation of boolean flags set from the environment.
fn truthy(v: &str) -> bool {
    !matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "f" | "no" | "n" | "off"
    )
}

/// Where falcon fetches its assets from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AssetSource {
    /// The well-known upstream URLs.
    Upstream,
    /// A falcon image server at the given base URL.
    ImgServer { base: String },
}

impl AssetSource {
    /// URL for an asset from the falcon assets bucket (images, OVMF).
    pub(crate) fn asset_url(&self, name: &str) -> String {
        match self {
            Self::Upstream => format!("{FALCON_ASSETS_URL}/{name}"),
            Self::ImgServer { base } => format!("{base}/v1/assets/{name}"),
        }
    }

    /// URL for a propolis falcon build artifact.
    pub(crate) fn propolis_url(&self, rev: &str, file: &str) -> String {
        match self {
            Self::Upstream => format!("{PROPOLIS_URL}/{rev}/{file}"),
            Self::ImgServer { base } => {
                format!("{base}/v1/propolis/{rev}/{file}")
            }
        }
    }

    /// Point users at the escape hatch when an image server misbehaves.
    pub(crate) fn annotate(&self, e: anyhow::Error) -> anyhow::Error {
        match self {
            Self::Upstream => e,
            Self::ImgServer { base } => e.context(format!(
                "failed to fetch from image server {base} \
                 (use --no-img-discovery or {NO_IMG_DISCOVERY_ENV}=1 to \
                 download from upstream instead)"
            )),
        }
    }
}

/// Lazily discovered [`AssetSource`]. Discovery runs on first use, so
/// operations that end up not downloading anything never depend on the image
/// server being reachable.
pub(crate) struct Assets {
    config: ImgServerConfig,
    log: Logger,
    source: tokio::sync::OnceCell<AssetSource>,
}

impl Assets {
    pub(crate) fn new(config: ImgServerConfig, log: Logger) -> Self {
        Self {
            config,
            log,
            source: tokio::sync::OnceCell::new(),
        }
    }

    pub(crate) async fn source(&self) -> anyhow::Result<&AssetSource> {
        self.source
            .get_or_try_init(|| discover(&self.config, &self.log))
            .await
    }
}

#[derive(Deserialize)]
struct DiscoverResponse {
    service: String,
    api_version: u32,
}

/// Decide where to fetch assets from.
///
/// - Discovery disabled: upstream.
/// - Default server name does not resolve: upstream.
/// - Explicit server does not resolve: error.
/// - Server resolves but does not answer discovery correctly: error. A broken
///   image server is never silently bypassed.
pub(crate) async fn discover(
    config: &ImgServerConfig,
    log: &Logger,
) -> anyhow::Result<AssetSource> {
    if !config.discovery {
        info!(
            log,
            "image server discovery disabled, using upstream sources"
        );
        return Ok(AssetSource::Upstream);
    }

    let (target, explicit) = match &config.server {
        Some(s) => (s.as_str(), true),
        None => (DEFAULT_IMG_SERVER, false),
    };
    let (host, port) = split_host_port(target)?;

    let resolved = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map(|mut addrs| addrs.next().is_some());
    match resolved {
        Ok(true) => {}
        Ok(false) | Err(_) if !explicit => {
            info!(
                log,
                "no image server found ({target} does not resolve), \
                 using upstream sources"
            );
            return Ok(AssetSource::Upstream);
        }
        Ok(false) => return Err(anyhow!("image server {target} not found")),
        Err(e) => {
            return Err(anyhow!("image server {target} not found: {e}"));
        }
    }

    let host = match host.parse::<Ipv6Addr>() {
        Ok(_) => format!("[{host}]"),
        Err(_) => host,
    };
    let base = format!("http://{host}:{port}");
    let source = AssetSource::ImgServer { base: base.clone() };

    probe(&base)
        .await
        .with_context(|| format!("image server discovery at {base} failed"))
        .map_err(|e| source.annotate(e))?;

    info!(log, "using image server {base}");
    Ok(source)
}

async fn probe(base: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()?;
    let resp = client.get(format!("{base}/v1/discover")).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("discovery returned {}", resp.status()));
    }
    let body = resp.bytes().await?;
    let d: DiscoverResponse = serde_json::from_slice(&body)
        .context("invalid discovery response; is this a falcon-img server?")?;
    if d.service != SERVICE_NAME {
        return Err(anyhow!("unexpected service {:?}", d.service));
    }
    if d.api_version != API_VERSION {
        return Err(anyhow!(
            "unsupported image server API version {} (expected {API_VERSION})",
            d.api_version
        ));
    }
    Ok(())
}

/// Split `host`, `host:port`, `[v6]`, `[v6]:port` or a bare IPv6 address.
fn split_host_port(s: &str) -> anyhow::Result<(String, u16)> {
    let s = s.trim();
    if s.is_empty() || s.contains('/') {
        return Err(anyhow!(
            "invalid image server {s:?}: expected host[:port]"
        ));
    }
    if let Ok(sa) = s.parse::<SocketAddr>() {
        return Ok((sa.ip().to_string(), sa.port()));
    }
    if let Ok(ip) = s.parse::<IpAddr>() {
        return Ok((ip.to_string(), DEFAULT_IMG_SERVER_PORT));
    }
    if let Some(v6) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let ip: Ipv6Addr = v6.parse().context("invalid IPv6 address")?;
        return Ok((ip.to_string(), DEFAULT_IMG_SERVER_PORT));
    }
    match s.rsplit_once(':') {
        Some((host, port)) => {
            let port = port
                .parse()
                .with_context(|| format!("invalid port in {s:?}"))?;
            Ok((host.to_owned(), port))
        }
        None => Ok((s.to_owned(), DEFAULT_IMG_SERVER_PORT)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port() {
        let p = |s| split_host_port(s).unwrap();
        assert_eq!(p("falcon-img"), ("falcon-img".into(), 7780));
        assert_eq!(p("falcon-img:80"), ("falcon-img".into(), 80));
        assert_eq!(p("10.0.0.1"), ("10.0.0.1".into(), 7780));
        assert_eq!(p("10.0.0.1:81"), ("10.0.0.1".into(), 81));
        assert_eq!(p("fd00::1"), ("fd00::1".into(), 7780));
        assert_eq!(p("[fd00::1]"), ("fd00::1".into(), 7780));
        assert_eq!(p("[fd00::1]:82"), ("fd00::1".into(), 82));
        assert!(split_host_port("http://falcon-img").is_err());
        assert!(split_host_port("falcon-img:x").is_err());
        assert!(split_host_port("").is_err());
    }

    #[test]
    fn urls() {
        let up = AssetSource::Upstream;
        assert_eq!(
            up.asset_url("helios-2.5_0.raw.xz"),
            "https://oxide-falcon-assets.s3.us-west-2.amazonaws.com/helios-2.5_0.raw.xz"
        );
        assert_eq!(
            up.propolis_url("abc", "propolis-server"),
            "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon/abc/propolis-server"
        );
        let srv = AssetSource::ImgServer {
            base: "http://h:7780".into(),
        };
        assert_eq!(
            srv.asset_url("OVMF_CODE.fd"),
            "http://h:7780/v1/assets/OVMF_CODE.fd"
        );
        assert_eq!(
            srv.propolis_url("abc", "propolis-server.sha256.txt"),
            "http://h:7780/v1/propolis/abc/propolis-server.sha256.txt"
        );
    }

    #[test]
    fn env_bools() {
        for v in ["", "0", "false", "False", "no", "off", "n", "f"] {
            assert!(!truthy(v), "{}", v);
        }
        for v in ["1", "true", "yes", "on"] {
            assert!(truthy(v), "{}", v);
        }
    }
}

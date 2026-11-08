// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2026 Oxide Computer Company

//! A caching proxy for the assets falcon downloads: base images, OVMF
//! firmware and propolis-server binaries.
//!
//! # Protocol
//!
//! All endpoints are plain HTTP `GET` requests.
//!
//! - `/v1/discover` returns a [`DiscoverResponse`] as JSON. Clients use this
//!   to confirm that the host they found is actually a falcon image server.
//! - `/v1/assets/{name}` returns `{name}` from the falcon assets bucket.
//! - `/v1/propolis/{rev}/{file}` returns `{file}` from the propolis falcon
//!   build artifacts for commit `{rev}`.
//!
//! # Cache layout
//!
//! Everything lives under a single cache directory (see [`DEFAULT_DIR`]).
//!
//! - `{dir}/{name}` holds assets, so an image can be provided out of band
//!   with `mv helios-2.5_0.raw.xz $(falcon-img dir)`.
//! - `{dir}/propolis/{rev}/{file}` holds propolis artifacts.
//! - `{dir}/.tmp` and `{dir}/.etag` are internal bookkeeping.
//!
//! # Freshness
//!
//! Base images (`*.raw.xz`) and propolis artifacts are immutable once
//! published, so a cached copy is always served without contacting upstream.
//! Any other asset (e.g. `OVMF_CODE.fd` and its digest) can change in place
//! upstream, so it is revalidated with a conditional request on every fetch.
//! When upstream cannot be reached, the cached copy is served.

use bytes::Bytes;
use futures::{Stream, StreamExt, TryStreamExt};
use http::{header, Method, Request, Response, StatusCode};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use slog::{error, info, warn, Logger};
use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_util::io::ReaderStream;

/// The name falcon resolves to find an image server, and the service name
/// reported by the discovery endpoint.
pub const SERVICE_NAME: &str = "falcon-img";

/// The version of the HTTP API described in the crate documentation.
pub const API_VERSION: u32 = 1;

/// The port falcon expects an image server to listen on.
pub const DEFAULT_PORT: u16 = 7780;

/// The default cache directory.
pub const DEFAULT_DIR: &str = "/var/falcon-img";

/// Where falcon assets (images, OVMF) come from.
pub const DEFAULT_ASSETS_UPSTREAM: &str =
    "https://oxide-falcon-assets.s3.us-west-2.amazonaws.com";

/// Where propolis-server builds for falcon come from.
pub const DEFAULT_PROPOLIS_UPSTREAM: &str =
    "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon";

const TMP_DIR: &str = ".tmp";
const ETAG_DIR: &str = ".etag";
const PROPOLIS_DIR: &str = "propolis";

/// Response body of `/v1/discover`.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoverResponse {
    /// Always [`SERVICE_NAME`].
    pub service: String,
    /// The API version this server speaks, see [`API_VERSION`].
    pub api_version: u32,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// Cache directory.
    pub dir: PathBuf,
    /// Base URL for falcon assets.
    pub assets_upstream: String,
    /// Base URL for propolis artifacts.
    pub propolis_upstream: String,
    /// Never contact upstream; only serve what is already in the cache.
    pub offline: bool,
}

impl Config {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            assets_upstream: DEFAULT_ASSETS_UPSTREAM.to_owned(),
            propolis_upstream: DEFAULT_PROPOLIS_UPSTREAM.to_owned(),
            offline: false,
        }
    }
}

type Body = UnsyncBoxBody<Bytes, io::Error>;

struct Server {
    config: Config,
    client: reqwest::Client,
    log: Logger,
    tmp_counter: AtomicU64,
}

/// An asset request after routing.
struct Asset {
    /// Location relative to the cache directory.
    rel: PathBuf,
    /// Where to get it if it's not cached.
    upstream: String,
    /// Whether the upstream copy can change.
    mutable: bool,
}

/// Prepare the cache directory. Leftover partial downloads from a previous
/// run are removed.
pub fn init_dir(dir: &Path) -> io::Result<()> {
    let tmp = dir.join(TMP_DIR);
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    std::fs::create_dir_all(&tmp)?;
    std::fs::create_dir_all(dir.join(ETAG_DIR))?;
    Ok(())
}

/// Serve requests on `listener` until an unrecoverable error occurs.
pub async fn serve(
    listener: TcpListener,
    config: Config,
    log: Logger,
) -> anyhow::Result<()> {
    init_dir(&config.dir)?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(60))
        .build()?;

    info!(log, "serving";
        "addr" => %listener.local_addr()?,
        "dir" => %config.dir.display(),
        "offline" => config.offline,
    );

    let server = Arc::new(Server {
        config,
        client,
        log,
        tmp_counter: AtomicU64::new(0),
    });

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                warn!(server.log, "accept: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let server = server.clone();
        tokio::spawn(async move {
            let log = server.log.clone();
            let svc = service_fn(move |req| {
                let server = server.clone();
                async move { Ok::<_, Infallible>(server.handle(req, peer).await) }
            });
            if let Err(e) = http1::Builder::new()
                .serve_connection(TokioIo::new(stream), svc)
                .await
            {
                warn!(log, "connection error"; "peer" => %peer, "error" => %e);
            }
        });
    }
}

/// Asset names and path segments are restricted to a conservative character
/// set so they can never escape the cache directory or collide with internal
/// bookkeeping directories.
fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+')
        })
}

impl Server {
    async fn handle(
        &self,
        req: Request<Incoming>,
        peer: SocketAddr,
    ) -> Response<Body> {
        let path = req.uri().path().to_owned();
        if req.method() != Method::GET {
            return text(StatusCode::METHOD_NOT_ALLOWED, "only GET supported");
        }
        let segments: Vec<&str> =
            path.trim_start_matches('/').split('/').collect();
        let asset = match segments.as_slice() {
            ["v1", "discover"] => return self.discover(),
            ["v1", "assets", name] if valid_segment(name) => Asset {
                rel: PathBuf::from(name),
                upstream: format!("{}/{name}", self.config.assets_upstream),
                mutable: !name.ends_with(".raw.xz"),
            },
            ["v1", "propolis", rev, file]
                if valid_segment(rev) && valid_segment(file) =>
            {
                Asset {
                    rel: [PROPOLIS_DIR, rev, file].iter().collect(),
                    upstream: format!(
                        "{}/{rev}/{file}",
                        self.config.propolis_upstream
                    ),
                    mutable: false,
                }
            }
            _ => return text(StatusCode::NOT_FOUND, "not found"),
        };
        let log = self.log.new(slog::o!("peer" => peer, "path" => path));
        self.fetch(asset, &log).await
    }

    fn discover(&self) -> Response<Body> {
        let body = serde_json::to_vec(&DiscoverResponse {
            service: SERVICE_NAME.to_owned(),
            api_version: API_VERSION,
        })
        .expect("serialize discover response");
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONTENT_LENGTH, body.len())
            .body(full(body))
            .unwrap()
    }

    async fn fetch(&self, asset: Asset, log: &Logger) -> Response<Body> {
        let path = self.config.dir.join(&asset.rel);
        let cached = tokio::fs::metadata(&path)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false);

        if cached && (!asset.mutable || self.config.offline) {
            info!(log, "cache hit");
            return serve_file(&path, log).await;
        }
        if self.config.offline {
            info!(log, "cache miss, offline");
            return text(
                StatusCode::NOT_FOUND,
                "not in cache and the image server is offline",
            );
        }

        let etag_path = self.etag_path(&asset);
        let mut req = self.client.get(&asset.upstream);
        if cached {
            if let Ok(etag) = tokio::fs::read_to_string(&etag_path).await {
                req = req.header(header::IF_NONE_MATCH, etag.trim());
            }
        }

        match req.send().await {
            Ok(resp) if resp.status() == StatusCode::NOT_MODIFIED && cached => {
                info!(log, "cache hit, revalidated");
                serve_file(&path, log).await
            }
            Ok(resp) if resp.status().is_success() => {
                info!(log, "cache miss, fetching";
                    "upstream" => &asset.upstream);
                let etag = asset
                    .mutable
                    .then(|| resp.headers().get(header::ETAG).cloned())
                    .flatten()
                    .and_then(|v| v.to_str().ok().map(str::to_owned));
                self.tee(resp, path, etag_path, etag, log.clone())
            }
            Ok(resp) if cached => {
                warn!(
                    log,
                    "upstream returned {}, serving cached copy",
                    resp.status()
                );
                serve_file(&path, log).await
            }
            Err(e) if cached => {
                warn!(log, "upstream unreachable, serving cached copy";
                    "error" => error_chain(&e));
                serve_file(&path, log).await
            }
            Ok(resp) => {
                let status = resp.status();
                warn!(log, "upstream returned {status}");
                // S3 answers 403 for missing keys when listing is not
                // permitted, so treat it as not found.
                if status == StatusCode::NOT_FOUND
                    || status == StatusCode::FORBIDDEN
                {
                    text(StatusCode::NOT_FOUND, "not found upstream")
                } else {
                    text(
                        StatusCode::BAD_GATEWAY,
                        &format!("upstream returned {status}"),
                    )
                }
            }
            Err(e) => {
                let e = error_chain(&e);
                error!(log, "upstream unreachable"; "error" => &e);
                text(
                    StatusCode::BAD_GATEWAY,
                    &format!("upstream unreachable: {e}"),
                )
            }
        }
    }

    fn etag_path(&self, asset: &Asset) -> PathBuf {
        let flat = asset.rel.to_string_lossy().replace('/', "_");
        self.config.dir.join(ETAG_DIR).join(flat)
    }

    fn tmp_path(&self) -> PathBuf {
        let n = self.tmp_counter.fetch_add(1, Ordering::Relaxed);
        self.config
            .dir
            .join(TMP_DIR)
            .join(format!("{}.{n}", std::process::id()))
    }

    /// Stream an upstream response to the client while writing it into the
    /// cache. The download runs in its own task so that it completes and
    /// populates the cache even if the client goes away. The file is only
    /// moved into place once it has been fully received.
    fn tee(
        &self,
        resp: reqwest::Response,
        dest: PathBuf,
        etag_path: PathBuf,
        etag: Option<String>,
        log: Logger,
    ) -> Response<Body> {
        let len = resp.content_length();
        let tmp = self.tmp_path();
        let (tx, rx) = mpsc::channel::<io::Result<Bytes>>(16);

        tokio::spawn(async move {
            let result: io::Result<u64> = async {
                let mut file = tokio::fs::File::create(&tmp).await?;
                let mut stream = resp.bytes_stream();
                let mut received = 0u64;
                let mut client_gone = false;
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(io::Error::other)?;
                    file.write_all(&chunk).await?;
                    received += chunk.len() as u64;
                    if !client_gone && tx.send(Ok(chunk)).await.is_err() {
                        client_gone = true;
                        info!(log, "client went away, still filling cache");
                    }
                }
                file.sync_all().await?;
                if let Some(len) = len {
                    if received != len {
                        return Err(io::Error::other(format!(
                            "short read from upstream: {received}/{len} bytes"
                        )));
                    }
                }
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::rename(&tmp, &dest).await?;
                match etag {
                    Some(etag) => tokio::fs::write(&etag_path, etag).await?,
                    None => {
                        let _ = tokio::fs::remove_file(&etag_path).await;
                    }
                }
                Ok(received)
            }
            .await;

            match result {
                Ok(n) => info!(log, "cached"; "bytes" => n),
                Err(e) => {
                    error!(log, "fetch failed"; "error" => %e);
                    let _ = tokio::fs::remove_file(&tmp).await;
                    // Abort the client's response so it does not mistake a
                    // partial body for a complete one.
                    let _ = tx.send(Err(e)).await;
                }
            }
        });

        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/octet-stream");
        if let Some(len) = len {
            builder = builder.header(header::CONTENT_LENGTH, len);
        }
        builder.body(stream_body(receiver_stream(rx))).unwrap()
    }
}

async fn serve_file(path: &Path, log: &Logger) -> Response<Body> {
    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            error!(log, "open cached file"; "error" => %e);
            return text(StatusCode::INTERNAL_SERVER_ERROR, "cache read error");
        }
    };
    let len = match file.metadata().await {
        Ok(m) => m.len(),
        Err(e) => {
            error!(log, "stat cached file"; "error" => %e);
            return text(StatusCode::INTERNAL_SERVER_ERROR, "cache read error");
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, len)
        .body(stream_body(ReaderStream::with_capacity(file, 1 << 20)))
        .unwrap()
}

/// Render an error with its causes; reqwest's top-level message alone rarely
/// says what went wrong.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        out.push_str(": ");
        out.push_str(&s.to_string());
        source = s.source();
    }
    out
}

fn receiver_stream(
    rx: mpsc::Receiver<io::Result<Bytes>>,
) -> impl Stream<Item = io::Result<Bytes>> {
    futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    })
}

fn stream_body<S>(s: S) -> Body
where
    S: Stream<Item = io::Result<Bytes>> + Send + 'static,
{
    StreamBody::new(s.map_ok(Frame::data)).boxed_unsync()
}

fn full(b: impl Into<Bytes>) -> Body {
    Full::new(b.into()).map_err(|e| match e {}).boxed_unsync()
}

fn text(status: StatusCode, msg: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(full(format!("{msg}\n")))
        .unwrap()
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2026 Oxide Computer Company

use bytes::Bytes;
use camino_tempfile::Utf8TempDir;
use falcon_img::{Config, DiscoverResponse, API_VERSION, SERVICE_NAME};
use http::{header, Request, Response, StatusCode};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use slog::Drain;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

/// A stand-in for S3/buildomat. Serves `files` with ETags and honors
/// `If-None-Match`, recording every request path.
#[derive(Clone, Default)]
struct Upstream {
    files: Arc<Mutex<HashMap<String, (Bytes, String)>>>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Upstream {
    fn put(&self, path: &str, body: &str, etag: &str) {
        self.files.lock().unwrap().insert(
            path.to_owned(),
            (Bytes::from(body.to_owned()), format!("\"{etag}\"")),
        );
    }

    fn hits(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| *p == path)
            .count()
    }

    async fn start(&self) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let this = this.clone();
                tokio::spawn(async move {
                    let svc = service_fn(move |req| {
                        let this = this.clone();
                        async move { Ok::<_, Infallible>(this.handle(req)) }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), svc)
                        .await;
                });
            }
        });
        addr
    }

    fn handle(&self, req: Request<Incoming>) -> Response<Full<Bytes>> {
        let path = req.uri().path().to_owned();
        self.requests.lock().unwrap().push(path.clone());
        let files = self.files.lock().unwrap();
        let Some((body, etag)) = files.get(&path) else {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::default())
                .unwrap();
        };
        if req
            .headers()
            .get(header::IF_NONE_MATCH)
            .is_some_and(|v| v == etag.as_str())
        {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, etag)
                .body(Full::default())
                .unwrap();
        }
        Response::builder()
            .header(header::ETAG, etag)
            .body(Full::new(body.clone()))
            .unwrap()
    }
}

struct Harness {
    upstream: Upstream,
    dir: Utf8TempDir,
    base: String,
    client: reqwest::Client,
}

impl Harness {
    async fn new(offline: bool) -> Self {
        Self::with_upstream(offline, None).await
    }

    /// `upstream_addr` overrides where the server looks for upstream, e.g. to
    /// point it at a closed port.
    async fn with_upstream(
        offline: bool,
        upstream_addr: Option<String>,
    ) -> Self {
        let upstream = Upstream::default();
        let up = match upstream_addr {
            Some(a) => a,
            None => format!("http://{}", upstream.start().await),
        };
        let dir = Utf8TempDir::new().unwrap();
        let config = Config {
            dir: dir.path().into(),
            assets_upstream: format!("{up}/assets"),
            propolis_upstream: format!("{up}/propolis"),
            offline,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(falcon_img::serve(listener, config, logger()));
        Self {
            upstream,
            dir,
            base,
            client: reqwest::Client::new(),
        }
    }

    async fn get(&self, path: &str) -> (StatusCode, String) {
        let resp = self
            .client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        (status, resp.text().await.unwrap())
    }

    /// Fetch and wait for the cache file to land. The cache write completes
    /// asynchronously with respect to the response.
    async fn get_and_cache(
        &self,
        path: &str,
        rel: &str,
    ) -> (StatusCode, String) {
        let r = self.get(path).await;
        for _ in 0..100 {
            if self.dir.path().join(rel).exists() {
                return r;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("{rel} never landed in cache");
    }

    fn cached(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.path().join(rel)).ok()
    }
}

fn logger() -> slog::Logger {
    let decorator = slog_term::PlainDecorator::new(slog_term::TestStdoutWriter);
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = std::sync::Mutex::new(drain).fuse();
    slog::Logger::root(drain, slog::o!())
}

#[tokio::test]
async fn discover() {
    let h = Harness::new(false).await;
    let (status, body) = h.get("/v1/discover").await;
    assert_eq!(status, StatusCode::OK);
    let d: DiscoverResponse = serde_json::from_str(&body).unwrap();
    assert_eq!(d.service, SERVICE_NAME);
    assert_eq!(d.api_version, API_VERSION);
}

#[tokio::test]
async fn image_is_cached_and_never_revalidated() {
    let h = Harness::new(false).await;
    h.upstream.put("/assets/helios_0.raw.xz", "image-v1", "a");

    let r = h
        .get_and_cache("/v1/assets/helios_0.raw.xz", "helios_0.raw.xz")
        .await;
    assert_eq!(r, (StatusCode::OK, "image-v1".into()));
    assert_eq!(h.cached("helios_0.raw.xz").as_deref(), Some("image-v1"));

    // Even if upstream changes, images are immutable: serve the cache.
    h.upstream.put("/assets/helios_0.raw.xz", "image-v2", "b");
    let r = h.get("/v1/assets/helios_0.raw.xz").await;
    assert_eq!(r, (StatusCode::OK, "image-v1".into()));
    assert_eq!(h.upstream.hits("/assets/helios_0.raw.xz"), 1);
}

#[tokio::test]
async fn out_of_band_image() {
    let h = Harness::new(false).await;
    std::fs::write(h.dir.path().join("airgap_0.raw.xz"), "manual").unwrap();
    let r = h.get("/v1/assets/airgap_0.raw.xz").await;
    assert_eq!(r, (StatusCode::OK, "manual".into()));
    assert_eq!(h.upstream.hits("/assets/airgap_0.raw.xz"), 0);
}

#[tokio::test]
async fn mutable_asset_is_revalidated() {
    let h = Harness::new(false).await;
    h.upstream.put("/assets/OVMF_CODE.fd", "fw-v1", "1");

    let r = h
        .get_and_cache("/v1/assets/OVMF_CODE.fd", "OVMF_CODE.fd")
        .await;
    assert_eq!(r, (StatusCode::OK, "fw-v1".into()));

    // Unchanged upstream: 304, served from cache.
    let r = h.get("/v1/assets/OVMF_CODE.fd").await;
    assert_eq!(r, (StatusCode::OK, "fw-v1".into()));
    assert_eq!(h.upstream.hits("/assets/OVMF_CODE.fd"), 2);

    // Changed upstream: new content is fetched and cached.
    h.upstream.put("/assets/OVMF_CODE.fd", "fw-v2", "2");
    let r = h.get("/v1/assets/OVMF_CODE.fd").await;
    assert_eq!(r, (StatusCode::OK, "fw-v2".into()));
    for _ in 0..100 {
        if h.cached("OVMF_CODE.fd").as_deref() == Some("fw-v2") {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("cache not updated");
}

#[tokio::test]
async fn propolis_artifacts() {
    let h = Harness::new(false).await;
    h.upstream
        .put("/propolis/abc123/propolis-server", "elf", "p");
    let rel = "propolis/abc123/propolis-server";
    let r = h
        .get_and_cache("/v1/propolis/abc123/propolis-server", rel)
        .await;
    assert_eq!(r, (StatusCode::OK, "elf".into()));
    let r = h.get("/v1/propolis/abc123/propolis-server").await;
    assert_eq!(r, (StatusCode::OK, "elf".into()));
    assert_eq!(h.upstream.hits("/propolis/abc123/propolis-server"), 1);
}

#[tokio::test]
async fn upstream_not_found() {
    let h = Harness::new(false).await;
    let (status, _) = h.get("/v1/assets/nope_0.raw.xz").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(h.cached("nope_0.raw.xz").is_none());
}

#[tokio::test]
async fn upstream_unreachable() {
    // Grab a free port and close it so connections are refused.
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let h =
        Harness::with_upstream(false, Some(format!("http://{closed}"))).await;

    let (status, _) = h.get("/v1/assets/x_0.raw.xz").await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);

    // A cached mutable asset is still served when upstream is down.
    std::fs::write(h.dir.path().join("OVMF_CODE.fd"), "fw").unwrap();
    let r = h.get("/v1/assets/OVMF_CODE.fd").await;
    assert_eq!(r, (StatusCode::OK, "fw".into()));
}

#[tokio::test]
async fn offline() {
    let h = Harness::new(true).await;
    h.upstream.put("/assets/x_0.raw.xz", "x", "x");
    let (status, _) = h.get("/v1/assets/x_0.raw.xz").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    std::fs::write(h.dir.path().join("OVMF_CODE.fd"), "fw").unwrap();
    let r = h.get("/v1/assets/OVMF_CODE.fd").await;
    assert_eq!(r, (StatusCode::OK, "fw".into()));
    assert!(h.upstream.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rejects_bad_paths() {
    let h = Harness::new(false).await;
    for p in [
        "/v1/assets/.tmp",
        "/v1/assets/.etag",
        "/v1/assets/..%2fetc%2fpasswd",
        "/v1/assets/a/b",
        "/v1/assets/",
        "/v1/propolis/../x",
        "/v1/propolis/abc",
        "/v2/discover",
    ] {
        let (status, _) = h.get(p).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{p}");
    }
    assert!(h.upstream.requests.lock().unwrap().is_empty());
}

use anyhow::Result;
use sha2::{Digest, Sha256};
use slog::{info, warn, Logger};
use std::fs;
use std::io;
use std::time::Duration;

use crate::imgsrv::AssetSource;

const OVMF: &str = "OVMF_CODE.fd";
const OVMF_DIGEST: &str = "OVMF_CODE.fd.sha256.txt";

pub(crate) async fn ensure_ovmf_fd(
    falcon_dir: &str,
    source: &AssetSource,
    log: &Logger,
) -> Result<()> {
    ensure_ovmf_fd_impl(falcon_dir, source, log)
        .await
        .map_err(|e| source.annotate(e))
}

async fn ensure_ovmf_fd_impl(
    falcon_dir: &str,
    source: &AssetSource,
    log: &Logger,
) -> Result<()> {
    let path = format!("{falcon_dir}/bin/{OVMF}");
    let Some(local_digest) = get_downloaded_ovmf_digest(&path)? else {
        info!(log, "ovmf fd not found");
        return download_ovmf(&path, source, log).await;
    };
    let remote_digest = get_expected_ovmf_digest(source, log).await?;
    if local_digest != remote_digest {
        info!(log,
            "ovmf digest '{local_digest}' does not match expected '{remote_digest}'"
        );
        return download_ovmf(&path, source, log).await;
    }
    Ok(())
}

async fn download_ovmf(
    path: &str,
    source: &AssetSource,
    log: &Logger,
) -> Result<()> {
    let url = source.asset_url(OVMF);
    info!(log, "downloading ovmf from {url}");
    crate::download_large_file(&url, path, log).await?;
    Ok(())
}

fn get_downloaded_ovmf_digest(path: &str) -> Result<Option<String>> {
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    let hash = base16ct::lower::encode_string(&hash);
    Ok(Some(hash))
}

async fn get_expected_ovmf_digest(
    source: &AssetSource,
    log: &Logger,
) -> Result<String> {
    let url = source.asset_url(OVMF_DIGEST);
    for _ in 0..9 {
        match get_expected_ovmf_digest_impl(&url).await {
            Ok(digest) => return Ok(digest),
            Err(e) => warn!(log, "{e}: retrying in 1 second"),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    get_expected_ovmf_digest_impl(&url).await
}

async fn get_expected_ovmf_digest_impl(url: &str) -> Result<String> {
    let client = reqwest::ClientBuilder::new()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()?;
    let response = client.get(url).send().await?.error_for_status()?;
    let text = response.text().await?;
    Ok(text.trim().to_owned())
}

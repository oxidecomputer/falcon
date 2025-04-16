use anyhow::Result;
use sha2::{Digest, Sha256};
use slog::{info, Logger};
use std::fs;
use std::io;

const OVMF_URL: &str =
    "https://oxide-falcon-assets.s3.us-west-2.amazonaws.com/OVMF_CODE.fd";
const OVMF_DIGEST_URL: &str =
    "https://oxide-falcon-assets.s3.us-west-2.amazonaws.com/OVMF_CODE.fd.sha256.txt";

pub(crate) async fn ensure_ovmf_fd(
    falcon_dir: &str,
    log: &Logger,
) -> Result<()> {
    let path = format!("{falcon_dir}/bin/OVMF_CODE.fd");
    let Some(local_digest) = get_downloaded_ovmf_digest(&path)? else {
        info!(log, "ovmf fd not found");
        return download_ovmf(&path, log).await;
    };
    let remote_digest = get_expected_ovmf_digest().await?;
    if local_digest != remote_digest {
        info!(log,
            "ovmf digest '{local_digest}' does not match expected '{remote_digest}'"
        );
        return download_ovmf(&path, log).await;
    }
    Ok(())
}

async fn download_ovmf(path: &str, log: &Logger) -> Result<()> {
    info!(log, "downloading ovmf");
    crate::download_large_file(OVMF_URL, path).await?;
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

async fn get_expected_ovmf_digest() -> Result<String> {
    let response = reqwest::get(OVMF_DIGEST_URL).await?;
    let text = response.text().await?;
    Ok(text.trim().to_owned())
}

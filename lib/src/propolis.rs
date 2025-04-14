use anyhow::Result;
use sha2::{Digest, Sha256};
use slog::{info, Logger};
use std::fs;
use std::io;

const PROPOLIS_SERVER_PATH: &str = ".falcon/bin/propolis-server";

pub(crate) async fn ensure_propolis_binary(
    rev: &str,
    log: &Logger,
) -> Result<()> {
    let Some(local_digest) = get_downloaded_propolis_version_shasum()? else {
        info!(log, "propolis-server binary not found");
        return download_propolis(rev, log).await;
    };
    let remote_digest = get_expected_propolis_version_shasum(rev).await?;
    if local_digest != remote_digest {
        info!(log,
            "propolis-server digest {local_digest} does not match expected {remote_digest}"
        );
        return download_propolis(rev, log).await;
    }
    Ok(())
}

async fn download_propolis(rev: &str, log: &Logger) -> Result<()> {
    info!(log, "downloading propolis server rev {rev}");
    let url = format!(
        "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon/{rev}/propolis-server"
    );
    crate::download_large_file(url.as_str(), PROPOLIS_SERVER_PATH).await
}

fn get_downloaded_propolis_version_shasum() -> Result<Option<String>> {
    let mut file = match fs::File::open(PROPOLIS_SERVER_PATH) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    let hash = base16ct::lower::encode_string(&hash);
    Ok(Some(hash))
}

async fn get_expected_propolis_version_shasum(rev: &str) -> Result<String> {
    let digest_url = format!(
        "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon/{rev}/propolis-server.sha256.txt"
    );

    let response = reqwest::get(digest_url).await?;
    Ok(response.text().await?)
}

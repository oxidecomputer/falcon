use anyhow::Result;
use sha2::{Digest, Sha256};
use slog::{info, Logger};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;

pub(crate) async fn ensure_propolis_binary(
    rev: &str,
    propolis_binary: &str,
    log: &Logger,
) -> Result<()> {
    let path = String::from(propolis_binary);
    let Some(local_digest) = get_downloaded_propolis_digest(&path)? else {
        info!(log, "propolis-server binary not found");
        return download_propolis(rev, &path, log).await;
    };
    let remote_digest = get_expected_propolis_digest(rev).await?;
    if local_digest != remote_digest {
        info!(log,
            "propolis-server digest '{local_digest}' does not match expected '{remote_digest}'"
        );
        return download_propolis(rev, &path, log).await;
    }
    Ok(())
}

async fn download_propolis(rev: &str, path: &str, log: &Logger) -> Result<()> {
    info!(
        log,
        "downloading propolis server rev {rev}, writing to {path}"
    );
    let url = format!(
        "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon/{rev}/propolis-server"
    );
    crate::download_large_file(url.as_str(), path, log).await?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn get_downloaded_propolis_digest(path: &str) -> Result<Option<String>> {
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

async fn get_expected_propolis_digest(rev: &str) -> Result<String> {
    let digest_url = format!(
        "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon/{rev}/propolis-server.sha256.txt"
    );

    let response = reqwest::get(digest_url).await?;
    let text = response.text().await?;
    Ok(text.trim().to_owned())
}

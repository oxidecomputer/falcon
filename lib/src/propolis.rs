use anyhow::Result;
use sha2::{Digest, Sha256};
use slog::{info, Logger};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;

//const PROPOLIS_SERVER_PATH: &str = ".falcon/bin/propolis-server";

pub(crate) async fn ensure_propolis_binary(
    rev: &str,
    falcon_dir: &str,
    log: &Logger,
) -> Result<()> {
    let path = format!("{falcon_dir}/bin/propolis-server");
    let Some(local_digest) = get_downloaded_propolis_shasum(&path)? else {
        info!(log, "propolis-server binary not found");
        return download_propolis(rev, &path, log).await;
    };
    let remote_digest = get_expected_propolis_shasum(rev).await?;
    if local_digest != remote_digest {
        info!(log,
            "propolis-server digest {local_digest} does not match expected {remote_digest}"
        );
        return download_propolis(rev, &path, log).await;
    }
    Ok(())
}

async fn download_propolis(rev: &str, path: &str, log: &Logger) -> Result<()> {
    info!(log, "downloading propolis server rev {rev}");
    let url = format!(
        "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon/{rev}/propolis-server"
    );
    crate::download_large_file(url.as_str(), path).await?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o655))?;
    Ok(())
}

fn get_downloaded_propolis_shasum(path: &str) -> Result<Option<String>> {
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

async fn get_expected_propolis_shasum(rev: &str) -> Result<String> {
    let digest_url = format!(
        "https://buildomat.eng.oxide.computer/public/file/oxidecomputer/propolis/falcon/{rev}/propolis-server.sha256.txt"
    );

    let response = reqwest::get(digest_url).await?;
    Ok(response.text().await?)
}

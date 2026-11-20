// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::cli::FetchAssetsArgs;
use crate::error::CiError;
use crate::github::{GitHub, GitHubWrite};

use super::fetch::{REVOKE_PATH, TOKEN_VARIABLE, api_client};
use super::release_assets_files::{Entry, Placement};

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const FORK_HINT: &str = "no token was created: a pull request from a fork gets none, and the cache did not hold the asset";

pub(super) fn fetch_assets(args: &FetchAssetsArgs) -> Result<String, CiError> {
    let token = std::env::var(TOKEN_VARIABLE)
        .ok()
        .filter(|token| !token.is_empty());
    let client = token
        .clone()
        .map(|token| api_client(token, DOWNLOAD_TIMEOUT));
    let outcome = run(args, client.as_ref().and_then(|client| client.as_ref().ok()));
    let revocation = client.map_or(Ok(()), |client| {
        client
            .and_then(|github| github.delete(REVOKE_PATH))
            .map_err(|e| {
                e.context("the release asset token could not be revoked; it expires within the hour")
            })
    });
    outcome?;
    revocation?;
    Ok(if token.is_some() {
        "Downloaded the release assets, verified their digests and revoked the token\n".to_owned()
    } else {
        "Verified the cached release assets without a token\n".to_owned()
    })
}

fn run(args: &FetchAssetsArgs, github: Option<&super::http::HttpGitHub>) -> Result<(), CiError> {
    let root = std::env::current_dir()
        .and_then(fs::canonicalize)
        .map_err(|e| CiError::new(format!("cannot read the workspace directory: {e}")))?;
    let entries: Vec<Entry> = args
        .asset
        .iter()
        .map(|text| Entry::parse(text))
        .collect::<Result<_, _>>()?;
    let placements: Vec<Placement> = entries
        .iter()
        .map(|entry| entry.place(&root, github.is_some()))
        .collect::<Result<_, _>>()?;
    for (entry, placement) in entries.iter().zip(&placements) {
        match github {
            Some(github) => download(github, entry, placement)?,
            None => verify_cached(entry, placement)?,
        }
    }
    Ok(())
}

fn verify_cached(entry: &Entry, placement: &Placement) -> Result<(), CiError> {
    let file = placement.file();
    match fs::symlink_metadata(file) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => {
            return Err(CiError::new(format!(
                "{} is not a regular file; the cached release asset is refused",
                file.display()
            )));
        }
        Err(_) => {
            return Err(CiError::new(format!(
                "{} is missing for {}: {FORK_HINT}",
                file.display(),
                entry.describe()
            )));
        }
    }
    let actual = hash_file(file)?;
    entry.expect_digest(&actual)
}

fn hash_file(file: &Path) -> Result<String, CiError> {
    let mut reader = fs::File::open(file)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| CiError::new(format!("cannot read {}: {e}", file.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn download(
    github: &super::http::HttpGitHub,
    entry: &Entry,
    placement: &Placement,
) -> Result<(), CiError> {
    let release = github.get(&entry.release_path())?;
    let id = asset_id(&release, entry)?;
    let reader = github.open_asset(&entry.asset_path(id))?;
    placement.store(reader, |actual| entry.expect_digest(actual))
}

fn asset_id(release: &str, entry: &Entry) -> Result<u64, CiError> {
    let json: serde_json::Value = serde_json::from_str(release)
        .map_err(|e| CiError::new(format!("release of {}: {e}", entry.describe())))?;
    let mut found = json["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|asset| asset["name"].as_str() == Some(entry.name()))
        .filter_map(|asset| asset["id"].as_u64());
    match (found.next(), found.next()) {
        (Some(id), None) => Ok(id),
        (None, _) => Err(CiError::new(format!(
            "the release has no asset named {}",
            entry.describe()
        ))),
        _ => Err(CiError::new(format!(
            "the release has more than one asset named {}",
            entry.describe()
        ))),
    }
}

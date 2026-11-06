// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use serde_json::Value;

use super::super::CollectorError;
use super::Context;

const TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

type Replacement = (Vec<u8>, Vec<u8>);

pub(super) fn prepare(context: &Context<'_>, repo: &Path) -> Result<(), CollectorError> {
    let vel = repo.join(".velnor");
    if let Ok(metadata) = fs::symlink_metadata(&vel) {
        if !metadata.file_type().is_dir() {
            return Err(CollectorError::operational(
                "fixture .velnor path must be a real directory",
            ));
        }
    } else {
        fs::create_dir(&vel).map_err(CollectorError::operational)?;
    }
    let config = vel.join("config.toml");
    if fs::symlink_metadata(&config).is_err() {
        fs::write(
            config,
            b"schema = 1\n\n[workflow]\ndefault_branch = \"main\"\n",
        )
        .map_err(CollectorError::operational)?;
    }
    let manifest = vel.join("release-manifest.json");
    if let Ok(metadata) = fs::symlink_metadata(&manifest) {
        if !metadata.file_type().is_file() {
            return Err(CollectorError::operational(
                "fixture release manifest must be a regular file",
            ));
        }
        return Ok(());
    }
    let placeholder = placeholder_digest(context.cli_sha256)?;
    let commit = "b".repeat(40);
    let mut entries = Vec::new();
    for target in TARGETS {
        let digest = if target == context.target {
            context.cli_sha256
        } else {
            &placeholder
        };
        entries.push(format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{0}/velnor-actions-{0}-{target}\",\"sha256\":\"{digest}\"}}",
            context.version
        ));
    }
    let text = format!(
        "{{\"schema\":1,\"version\":\"{}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{}]}}\n",
        context.version,
        commit,
        entries.join(",")
    );
    fs::write(manifest, text).map_err(CollectorError::operational)
}

pub(super) fn replacements(manifest: &Path) -> Result<Vec<Replacement>, CollectorError> {
    let bytes = fs::read(manifest).map_err(CollectorError::operational)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(CollectorError::operational)?;
    let commit = value
        .get("commit")
        .and_then(Value::as_str)
        .filter(|value| is_hex(value, 40))
        .ok_or_else(|| {
            CollectorError::operational("validated manifest has a malformed source commit")
        })?;
    let mut replacements = Vec::new();
    for target in value
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let digest = target
            .get("sha256")
            .and_then(Value::as_str)
            .filter(|value| is_hex(value, 64))
            .ok_or_else(|| {
                CollectorError::operational("validated manifest has a malformed target digest")
            })?;
        replacements.push((digest.as_bytes().to_vec(), vec![b'a'; 64]));
    }
    replacements.push((commit.as_bytes().to_vec(), vec![b'b'; 40]));
    Ok(replacements)
}

fn placeholder_digest(actual: &str) -> Result<String, CollectorError> {
    for byte in b"abcdef01" {
        let value = (*byte as char).to_string().repeat(64);
        if value != actual {
            return Ok(value);
        }
    }
    Err(CollectorError::operational(
        "could not choose a fixture-only asset digest",
    ))
}

fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

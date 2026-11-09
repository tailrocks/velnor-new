// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::publication::github::read::get_json;
use crate::{OrchestratorError, internal::internal};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use sha1_checked::Digest as _;
use sha2::{Digest as _, Sha256};
use std::time::Duration;
use velnor_actions_mise::CheckDeadline;

const REPOSITORY: &str = "repos/tailrocks/velnor-new";
const REF: &str = "heads/velnor-mac-proof-inbox";
const TRANSPORT_LIMIT: usize = 4 * 1024 * 1024;
const ENVELOPE_LIMIT: usize = 2 * 1024 * 1024;
const BUNDLE_LIMIT: usize = 512 * 1024;

#[derive(Deserialize)]
struct GitRef {
    #[serde(rename = "ref")]
    reference: String,
    object: GitObject,
}
#[derive(Deserialize)]
struct GitObject {
    sha: String,
    #[serde(rename = "type")]
    kind: String,
}
#[derive(Deserialize)]
struct GitCommit {
    sha: String,
    tree: GitObjectSha,
}
#[derive(Deserialize)]
struct GitObjectSha {
    sha: String,
}
#[derive(Deserialize)]
struct GitTree {
    sha: String,
    truncated: bool,
    tree: Vec<GitEntry>,
}
#[derive(Deserialize)]
struct GitEntry {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}
#[derive(Deserialize)]
struct GitBlob {
    sha: String,
    size: usize,
    encoding: String,
    content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Transport {
    schema: String,
    envelope_base64: String,
    sigstore_bundle_base64: String,
}

pub(super) struct Proof {
    pub(super) envelope: Vec<u8>,
    pub(super) bundle: Vec<u8>,
}

pub(super) fn discover(
    run: u64,
    attempt: u32,
    manifest_sha: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Proof, OrchestratorError> {
    if run == 0 || attempt == 0 || !hex(manifest_sha, 64) {
        return Err(invalid());
    }
    let mut checked_commit = None;
    loop {
        let (proof, commit) = scan(
            run,
            attempt,
            manifest_sha,
            token,
            deadline,
            checked_commit.as_deref(),
        )?;
        if let Some(proof) = proof {
            return Ok(proof);
        }
        checked_commit = commit;
        let remaining = deadline.remaining().map_err(|_| invalid())?;
        if remaining.is_zero() {
            return Err(invalid());
        }
        std::thread::sleep(Duration::from_secs(15).min(remaining));
    }
}

fn scan(
    run: u64,
    attempt: u32,
    manifest_sha: &str,
    token: &str,
    deadline: CheckDeadline,
    checked_commit: Option<&str>,
) -> Result<(Option<Proof>, Option<String>), OrchestratorError> {
    let references: Vec<GitRef> = get_json(
        &format!("{REPOSITORY}/git/matching-refs/{REF}"),
        token,
        deadline,
    )?;
    if references.len() > 4096 {
        return Err(invalid());
    }
    let full_ref = format!("refs/{REF}");
    let mut matching = references
        .iter()
        .filter(|reference| reference.reference == full_ref);
    let Some(reference) = matching.next() else {
        return Ok((None, None));
    };
    if matching.next().is_some()
        || reference.object.kind != "commit"
        || !hex(&reference.object.sha, 40)
    {
        return Err(invalid());
    }
    if checked_commit == Some(reference.object.sha.as_str()) {
        return Ok((None, checked_commit.map(str::to_owned)));
    }
    let commit: GitCommit = get_json(
        &format!("{REPOSITORY}/git/commits/{}", reference.object.sha),
        token,
        deadline,
    )?;
    if commit.sha != reference.object.sha || !hex(&commit.tree.sha, 40) {
        return Err(invalid());
    }
    let tree: GitTree = get_json(
        &format!("{REPOSITORY}/git/trees/{}?recursive=1", commit.tree.sha),
        token,
        deadline,
    )?;
    let entry = select_entry(tree, &commit.tree.sha, run, attempt, manifest_sha)?;
    let Some(entry) = entry else {
        return Ok((None, Some(commit.sha)));
    };
    let digest = entry
        .path
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".json"))
        .ok_or_else(invalid)?;
    Ok((
        Some(read_proof(&entry, digest, token, deadline)?),
        Some(commit.sha),
    ))
}

fn select_entry(
    tree: GitTree,
    expected_sha: &str,
    run: u64,
    attempt: u32,
    manifest_sha: &str,
) -> Result<Option<GitEntry>, OrchestratorError> {
    if tree.sha != expected_sha || tree.truncated || tree.tree.len() > 20_000 {
        return Err(invalid());
    }
    let prefix = format!("proofs/{run}/{attempt}/{manifest_sha}/");
    let mut paths = std::collections::BTreeSet::new();
    let mut directories = std::collections::BTreeSet::new();
    for entry in &tree.tree {
        if !paths.insert(entry.path.clone()) || !valid_tree_entry(entry) {
            return Err(invalid());
        }
        if entry.kind == "tree" {
            directories.insert(entry.path.clone());
        }
    }
    let mut candidate = None;
    for entry in tree.tree {
        if let Some((parent, _)) = entry.path.rsplit_once('/')
            && !directories.contains(parent)
        {
            return Err(invalid());
        }
        if entry.kind == "blob"
            && entry.path.starts_with(&prefix)
            && candidate.replace(entry).is_some()
        {
            return Err(invalid());
        }
    }
    Ok(candidate)
}

fn valid_tree_entry(entry: &GitEntry) -> bool {
    if !hex(&entry.sha, 40) {
        return false;
    }
    let parts: Vec<_> = entry.path.split('/').collect();
    if parts.first() != Some(&"proofs") {
        return false;
    }
    let directory = entry.kind == "tree" && entry.mode == "040000";
    match parts.as_slice() {
        [_] => directory,
        [_, run] => directory && canonical_number::<u64>(run),
        [_, run, attempt] => {
            directory && canonical_number::<u64>(run) && canonical_number::<u32>(attempt)
        }
        [_, run, attempt, manifest] => {
            directory
                && canonical_number::<u64>(run)
                && canonical_number::<u32>(attempt)
                && hex(manifest, 64)
        }
        [_, run, attempt, manifest, name] => {
            entry.kind == "blob"
                && entry.mode == "100644"
                && canonical_number::<u64>(run)
                && canonical_number::<u32>(attempt)
                && hex(manifest, 64)
                && name
                    .strip_suffix(".json")
                    .is_some_and(|digest| hex(digest, 64))
        }
        _ => false,
    }
}

fn canonical_number<T: std::str::FromStr + std::fmt::Display + PartialEq + Default>(
    value: &str,
) -> bool {
    value
        .parse::<T>()
        .is_ok_and(|number| number != T::default() && number.to_string() == value)
}

fn read_proof(
    entry: &GitEntry,
    digest: &str,
    token: &str,
    deadline: CheckDeadline,
) -> Result<Proof, OrchestratorError> {
    let blob: GitBlob = get_json(
        &format!("{REPOSITORY}/git/blobs/{}", entry.sha),
        token,
        deadline,
    )?;
    decode_proof(&blob, entry, digest)
}

fn decode_proof(
    blob: &GitBlob,
    entry: &GitEntry,
    digest: &str,
) -> Result<Proof, OrchestratorError> {
    if blob.sha != entry.sha
        || blob.encoding != "base64"
        || blob.size == 0
        || blob.size > TRANSPORT_LIMIT
    {
        return Err(invalid());
    }
    let clean: String = blob
        .content
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .collect();
    let bytes = STANDARD.decode(clean.as_bytes()).map_err(|_| invalid())?;
    if STANDARD.encode(&bytes) != clean
        || bytes.len() != blob.size
        || git_blob_sha(&bytes)? != entry.sha
        || hex_digest(&Sha256::digest(&bytes)) != digest
    {
        return Err(invalid());
    }
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, TRANSPORT_LIMIT)
            .map_err(|_| invalid())?;
    let transport: Transport = serde_json::from_value(value).map_err(|_| invalid())?;
    if transport.schema != "velnor.runner-image-mac-proof-transport.v1" {
        return Err(invalid());
    }
    let envelope = decode(&transport.envelope_base64, ENVELOPE_LIMIT)?;
    let bundle = decode(&transport.sigstore_bundle_base64, BUNDLE_LIMIT)?;
    Ok(Proof { envelope, bundle })
}

fn decode(value: &str, limit: usize) -> Result<Vec<u8>, OrchestratorError> {
    let bytes = STANDARD.decode(value).map_err(|_| invalid())?;
    if bytes.is_empty() || bytes.len() > limit || STANDARD.encode(&bytes) != value {
        return Err(invalid());
    }
    Ok(bytes)
}
fn git_blob_sha(bytes: &[u8]) -> Result<String, OrchestratorError> {
    let mut sha = sha1_checked::Sha1::default();
    sha.update(format!("blob {}\0", bytes.len()).as_bytes());
    sha.update(bytes);
    let result = sha.try_finalize();
    if result.has_collision() {
        return Err(invalid());
    }
    Ok(hex_digest(result.hash()))
}
fn hex_digest(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|v| {
            [
                char::from(ALPHABET[usize::from(v >> 4)]),
                char::from(ALPHABET[usize::from(v & 15)]),
            ]
        })
        .collect()
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn invalid() -> OrchestratorError {
    internal("mac_proof_inbox_invalid")
}

#[cfg(test)]
#[path = "mac_inbox_tests.rs"]
mod tests;

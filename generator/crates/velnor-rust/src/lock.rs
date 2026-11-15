// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256 as Hasher};
use velnor_model::{CommitSha, HelperRelease, HelperVersion, Platform, RepoPath, Sha256};

pub const LOCK_PATH: RepoPath = RepoPath::from_static(".velnor/rust-ci.lock");
const RELEASES: &str = "https://github.com/tailrocks/velnor-new/releases/download";
const CI: &str = "velnor-ci";
const GENERATOR: &str = "velnor-gen";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockError(String);

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LockError {}

impl LockError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    version: String,
    commit: String,
    #[serde(rename = "velnor-ci")]
    ci: BTreeMap<String, String>,
    #[serde(rename = "velnor-gen")]
    generator: BTreeMap<String, String>,
}

pub fn release_version(text: &str) -> Result<HelperVersion, LockError> {
    let numeric = text.split('.').count() == 3
        && text
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    if !numeric {
        return Err(LockError(format!(
            "version {text:?} must be MAJOR.MINOR.PATCH, digits only"
        )));
    }
    HelperVersion::new(text).map_err(|error| LockError(error.to_string()))
}

#[must_use]
pub fn sums_url(version: &HelperVersion) -> String {
    format!("{RELEASES}/rust-ci-v{version}/SHA256SUMS")
}

#[must_use]
pub fn commit_url(version: &HelperVersion) -> String {
    format!("{RELEASES}/rust-ci-v{version}/COMMIT")
}

pub fn pin_lock(version: &str, commit: &str, sums: &str) -> Result<String, LockError> {
    let version = release_version(version)?;
    let commit_text = commit;
    let commit = release_commit(commit.trim_end())?;
    let listed = listed_sums(sums)?;
    check_commit_digest(commit_text, &listed)?;
    let lock = Lock {
        version: version.to_string(),
        commit: commit.to_string(),
        ci: digests(CI, &listed)?,
        generator: digests(GENERATOR, &listed)?,
    };
    toml::to_string(&lock).map_err(|error| LockError(error.to_string()))
}

fn check_commit_digest(commit: &str, listed: &BTreeMap<&str, &str>) -> Result<(), LockError> {
    let expected = listed
        .get("COMMIT")
        .ok_or_else(|| LockError("SHA256SUMS lists no COMMIT".to_owned()))?;
    let actual = Hasher::digest(commit.as_bytes())
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect::<String>();
    if actual == *expected {
        Ok(())
    } else {
        Err(LockError(format!(
            "COMMIT has SHA-256 {actual} but SHA256SUMS lists {expected}"
        )))
    }
}

fn release_commit(text: &str) -> Result<CommitSha, LockError> {
    CommitSha::new(text)
        .map_err(|_| LockError(format!("commit {text:?} must be 40 lowercase hex digits")))
}

pub(crate) fn parse_lock(bytes: &[u8]) -> Result<HelperRelease, LockError> {
    let text = std::str::from_utf8(bytes).map_err(|error| LockError(error.to_string()))?;
    let lock: Lock = toml::from_str(text).map_err(|error| LockError(error.to_string()))?;
    let version = release_version(&lock.version)?;
    let commit = release_commit(&lock.commit)?;
    check_digests(&lock.generator)?;
    check_digests(&lock.ci)?;
    let mut digests = BTreeMap::new();
    for platform in Platform::ALL {
        digests.insert(platform, digest(&lock.ci, platform)?);
    }
    HelperRelease::new(version, commit, digests).map_err(|error| LockError(error.to_string()))
}

fn listed_sums(sums: &str) -> Result<BTreeMap<&str, &str>, LockError> {
    let mut listed = BTreeMap::new();
    for (sum, name) in sums.lines().filter_map(|line| line.split_once("  ")) {
        if listed.insert(name, sum).is_some() {
            return Err(LockError(format!("SHA256SUMS lists {name} twice")));
        }
    }
    Ok(listed)
}

fn digests(
    program: &str,
    listed: &BTreeMap<&str, &str>,
) -> Result<BTreeMap<String, String>, LockError> {
    let mut found = BTreeMap::new();
    for platform in Platform::ALL {
        let name = format!("{program}-{}", platform.file_suffix());
        let sum = listed
            .get(name.as_str())
            .ok_or_else(|| LockError(format!("SHA256SUMS lists no {name}")))?;
        Sha256::new(*sum)
            .map_err(|_| LockError(format!("SHA256SUMS has no SHA-256 for {name}")))?;
        found.insert(platform.file_suffix().to_owned(), (*sum).to_owned());
    }
    Ok(found)
}

fn check_digests(found: &BTreeMap<String, String>) -> Result<(), LockError> {
    if found.len() != Platform::ALL.len() {
        return Err(LockError(format!(
            "a program must list exactly the platforms {}",
            Platform::ALL.map(Platform::file_suffix).join(", ")
        )));
    }
    Platform::ALL
        .into_iter()
        .try_for_each(|platform| digest(found, platform).map(drop))
}

fn digest(found: &BTreeMap<String, String>, platform: Platform) -> Result<Sha256, LockError> {
    let sum = found
        .get(platform.file_suffix())
        .ok_or_else(|| LockError(format!("no SHA-256 for {}", platform.file_suffix())))?;
    Sha256::new(sum.as_str()).map_err(|_| {
        LockError(format!(
            "the SHA-256 for {} is not 64 lowercase hex digits",
            platform.file_suffix()
        ))
    })
}

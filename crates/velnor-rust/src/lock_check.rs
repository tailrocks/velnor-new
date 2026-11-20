// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The offline and online checks of the lock's `[velnor-gen]` entry, used by `velnor-gen check`.

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{HelperVersion, Platform};

use crate::lock::{
    CI, GENERATOR, Lock, LockError, check_digests, pin_lock, release_commit, release_version,
};

/// The commit a `velnor-gen` build without `VELNOR_COMMIT` reports.
const UNSTAMPED: &str = "unknown";

/// The velnor-gen binary that runs `check`, as its own build describes it.
#[derive(Debug, Clone, Copy)]
pub struct RunningGenerator<'a> {
    pub version: &'a str,
    /// The `VELNOR_COMMIT` the binary was built with; `unknown` for a local build.
    pub commit: &'a str,
    /// This binary's platform, when the lock lists one.
    pub platform: Option<Platform>,
    /// The SHA-256 of the executable file, lowercase hex.
    pub sha256: Option<&'a str>,
}

#[must_use]
pub fn running_platform() -> Option<Platform> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(Platform::LinuxX64),
        ("linux", "aarch64") => Some(Platform::LinuxArm64),
        ("macos", "aarch64") => Some(Platform::MacosArm64),
        _ => None,
    }
}

/// The version of the lock's helper release.
pub fn lock_version(bytes: &[u8]) -> Result<HelperVersion, LockError> {
    release_version(&parse(bytes)?.version)
}

fn parse(bytes: &[u8]) -> Result<Lock, LockError> {
    let text = std::str::from_utf8(bytes).map_err(|error| LockError::new(error.to_string()))?;
    toml::from_str(text).map_err(|error| LockError::new(error.to_string()))
}

fn named(section: &str, error: &LockError) -> LockError {
    LockError::new(format!("[{section}] {error}"))
}

/// Offline: the `[velnor-gen]` entry agrees with the lock's own version and commit and, when
/// the running binary is a release build of the lock's commit, with that binary's bytes.
/// A local build (`unknown` commit) is not compared, since its digest is not the published one.
/// A newer binary that is not the locked release is not compared either.
pub fn check_generator_entry(bytes: &[u8], running: RunningGenerator<'_>) -> Result<(), LockError> {
    let lock = parse(bytes)?;
    release_version(&lock.version)?;
    let commit = release_commit(&lock.commit)?;
    check_digests(&lock.generator).map_err(|error| named(GENERATOR, &error))?;
    if running.commit == UNSTAMPED {
        return Ok(());
    }
    if running.commit != commit.as_str() {
        if running.version == lock.version {
            return Err(LockError::new(format!(
                "commit: the lock pins {commit} but this velnor-gen {} was built from {}",
                running.version, running.commit
            )));
        }
        return Ok(());
    }
    if running.version != lock.version {
        return Err(LockError::new(format!(
            "version: the lock pins {} but this velnor-gen built from {commit} is {}",
            lock.version, running.version
        )));
    }
    if let (Some(platform), Some(sha)) = (running.platform, running.sha256)
        && let Some(listed) = lock.generator.get(platform.file_suffix())
        && listed != sha
    {
        return Err(LockError::new(format!(
            "[{GENERATOR}] {} digest is {listed} but this binary hashes to {sha}",
            platform.file_suffix()
        )));
    }
    Ok(())
}

/// Online: the lock is what the release publishes. Pins the release again from its own
/// `SHA256SUMS`, `COMMIT` and `BINARY-SHA256SUMS` and names the first field that differs.
pub fn verify_release(
    bytes: &[u8],
    sums: &str,
    commit: &str,
    binary_sums: Option<&str>,
) -> Result<(), LockError> {
    let lock = parse(bytes)?;
    let release = parse(pin_lock(&lock.version, commit, sums, binary_sums)?.as_bytes())?;
    if lock.commit != release.commit {
        return Err(LockError::new(format!(
            "commit: the lock pins {} but the release's COMMIT is {}",
            lock.commit, release.commit
        )));
    }
    compare_section(CI, Some(&lock.ci), Some(&release.ci))?;
    compare_section(
        &format!("{CI}-binary"),
        lock.ci_binary.as_ref(),
        release.ci_binary.as_ref(),
    )?;
    compare_section(GENERATOR, Some(&lock.generator), Some(&release.generator))
}

fn compare_section(
    section: &str,
    lock: Option<&BTreeMap<String, String>>,
    release: Option<&BTreeMap<String, String>>,
) -> Result<(), LockError> {
    let empty = BTreeMap::new();
    let (lock, release) = (lock.unwrap_or(&empty), release.unwrap_or(&empty));
    let platforms: BTreeSet<&String> = lock.keys().chain(release.keys()).collect();
    for platform in platforms {
        let (have, published) = (lock.get(platform), release.get(platform));
        if have != published {
            return Err(LockError::new(format!(
                "[{section}] {platform} digest is {} but the release lists {}",
                have.map_or("none", String::as_str),
                published.map_or("none", String::as_str)
            )));
        }
    }
    Ok(())
}

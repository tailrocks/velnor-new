// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{HelperVersion, Platform};

use crate::lock::{
    CI, GENERATOR, GENERATOR_BINARY_RELEASE, Lock, LockError, check_digests, named, pin_lock,
    release_commit, release_version,
};

const UNSTAMPED: &str = "unknown";

const FIRST_ARCHIVE_RELEASE: (u64, u64, u64) = (0, 1, 94);

#[derive(Debug, Clone, Copy)]
pub struct RunningGenerator<'a> {
    pub version: &'a str,
    pub commit: &'a str,
    pub platform: Option<Platform>,
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

pub fn lock_version(bytes: &[u8]) -> Result<HelperVersion, LockError> {
    release_version(&parse(bytes)?.version)
}

fn parse(bytes: &[u8]) -> Result<Lock, LockError> {
    let text = std::str::from_utf8(bytes).map_err(|error| LockError::new(error.to_string()))?;
    toml::from_str(text).map_err(|error| LockError::new(error.to_string()))
}

pub fn check_generator_entry(bytes: &[u8], running: RunningGenerator<'_>) -> Result<(), LockError> {
    let lock = parse(bytes)?;
    release_version(&lock.version)?;
    let commit = release_commit(&lock.commit)?;
    check_digests(&lock.generator).map_err(|error| named(GENERATOR, &error))?;
    let binary = binary_entry(&lock)?;
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
    if let (Some(platform), Some(sha), Some((section, digests))) =
        (running.platform, running.sha256, binary)
        && let Some(listed) = digests.get(platform.file_suffix())
        && listed != sha
    {
        return Err(LockError::new(format!(
            "[{section}] {} digest is {listed} but this binary hashes to {sha}",
            platform.file_suffix()
        )));
    }
    Ok(())
}

pub fn check_generator_binary(
    bytes: &[u8],
    running: RunningGenerator<'_>,
) -> Result<(), LockError> {
    check_generator_entry(bytes, running)?;
    binary_entry(&parse(bytes)?)?.map(drop).ok_or_else(|| {
        LockError::new(format!(
            "the lock lists no velnor-gen binary digests; {}",
            repin_hint()
        ))
    })
}

type BinaryEntry<'a> = (String, &'a BTreeMap<String, String>);

fn binary_entry(lock: &Lock) -> Result<Option<BinaryEntry<'_>>, LockError> {
    let archive = check_format(lock)?;
    let reads = release_version(&lock.version)?.at_least(GENERATOR_BINARY_RELEASE);
    let section = format!("{GENERATOR}-binary");
    match (archive, reads, lock.generator_binary.as_ref()) {
        (false, _, None) => Ok(Some((GENERATOR.to_owned(), &lock.generator))),
        (false, _, Some(_)) => Err(LockError::new(format!(
            "[{section}] is not allowed: release {} publishes raw binaries only",
            lock.version
        ))),
        (true, false, Some(_)) => Err(LockError::new(format!(
            "[{section}] is not allowed: release {} does not read it",
            lock.version
        ))),
        (true, false, None) => Ok(None),
        (true, true, None) => Err(LockError::new(format!(
            "[{section}] is missing: release {} lists the velnor-gen binary digests, so the lock must list them; {}",
            lock.version,
            repin_hint()
        ))),
        (true, true, Some(binary)) => {
            check_digests(binary).map_err(|error| named(&section, &error))?;
            Ok(Some((section, binary)))
        }
    }
}

fn repin_hint() -> String {
    let (major, minor, patch) = GENERATOR_BINARY_RELEASE;
    format!("re-pin with `velnor-gen pin-helper {major}.{minor}.{patch}` or later")
}

fn check_format(lock: &Lock) -> Result<bool, LockError> {
    let archive = release_version(&lock.version)?.at_least(FIRST_ARCHIVE_RELEASE);
    match (archive, lock.ci_binary.as_ref()) {
        (true, Some(binary)) => {
            check_digests(binary).map_err(|error| named(&format!("{CI}-binary"), &error))?;
            Ok(true)
        }
        (true, None) => Err(LockError::new(format!(
            "[{CI}-binary] is missing: release {} publishes archives, so the lock must list the binary digests",
            lock.version
        ))),
        (false, Some(_)) => Err(LockError::new(format!(
            "[{CI}-binary] is not allowed: release {} publishes raw binaries only",
            lock.version
        ))),
        (false, None) => Ok(false),
    }
}

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
    compare_section(GENERATOR, Some(&lock.generator), Some(&release.generator))?;
    let section = format!("{GENERATOR}-binary");
    if lock.generator_binary.is_none() && release.generator_binary.is_some() {
        return Err(LockError::new(format!(
            "[{section}] is missing: release {} lists the velnor-gen binary digests; {}",
            lock.version,
            repin_hint()
        )));
    }
    compare_section(
        &section,
        lock.generator_binary.as_ref(),
        release.generator_binary.as_ref(),
    )
    .map_err(|error| LockError::new(format!("{error}; {}", repin_hint())))
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

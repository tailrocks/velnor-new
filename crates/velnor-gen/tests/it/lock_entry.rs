// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::Platform;
use velnor_rust::{RunningGenerator, check_generator_entry, pin_lock, verify_release};

use crate::lock::{COMMIT_FILE, SUMS};
use crate::lock_archive::archive_lock_at;

type TestResult = Result<(), Box<dyn Error>>;

/// The commit `COMMIT_FILE` names, as a lock stores it.
const COMMIT: &str = "1390e6f6a4b64e1f33dbddfbeedcd097db32ec36";
/// The release's `velnor-gen-linux-x64` digest in `SUMS`.
const GEN_LINUX_X64: &str = "61ec20debeae57cce969660e66c72315c59ce3cee84659540aee1942aad52628";
const OTHER_COMMIT: &str = "2793cf84b5924e2542a03b0fb2580e7395b2f4cb";

fn lock() -> Result<String, Box<dyn Error>> {
    Ok(pin_lock("0.1.81", COMMIT_FILE, SUMS, None)?)
}

/// This binary as a release build of the lock's own release: same version, same commit, and
/// the bytes whose digest the lock lists.
fn release_binary<'a>() -> RunningGenerator<'a> {
    RunningGenerator {
        version: "0.1.81",
        commit: COMMIT,
        platform: Some(Platform::LinuxX64),
        sha256: Some(GEN_LINUX_X64),
    }
}

fn check(text: &str, running: RunningGenerator<'_>) -> Result<(), String> {
    check_generator_entry(text.as_bytes(), running).map_err(|error| error.to_string())
}

#[test]
fn a_clean_lock_passes_for_a_local_build_that_stamps_no_commit() -> TestResult {
    let local = RunningGenerator {
        version: "0.1.106",
        commit: "unknown",
        platform: Some(Platform::LinuxX64),
        sha256: None,
    };
    assert_eq!(check(&lock()?, local), Ok(()));
    Ok(())
}

#[test]
fn a_clean_lock_passes_for_the_release_binary_it_pins() -> TestResult {
    assert_eq!(check(&lock()?, release_binary()), Ok(()));
    Ok(())
}

#[test]
fn an_archive_lock_is_not_compared_with_the_bytes_of_the_running_binary() -> TestResult {
    let archive = archive_lock_at("0.1.108")?;
    let commit = archive
        .lines()
        .find_map(|line| line.strip_prefix("commit = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or("the lock names no commit")?;
    let zeros = "0".repeat(64);
    let release = RunningGenerator {
        version: "0.1.108",
        commit,
        platform: Some(Platform::LinuxX64),
        sha256: Some(&zeros),
    };
    assert_eq!(check(&archive, release), Ok(()));
    Ok(())
}

#[test]
fn a_newer_release_binary_passes_an_older_pinned_lock() -> TestResult {
    let newer = RunningGenerator {
        version: "0.1.90",
        commit: OTHER_COMMIT,
        platform: Some(Platform::LinuxX64),
        sha256: Some("0000000000000000000000000000000000000000000000000000000000000000"),
    };
    assert_eq!(check(&lock()?, newer), Ok(()));
    Ok(())
}

#[test]
fn a_tampered_digest_is_refused_and_the_message_names_the_field() -> TestResult {
    let tampered = lock()?.replacen(GEN_LINUX_X64, &"0".repeat(64), 1);
    let error = check(&tampered, release_binary()).err().ok_or("accepted")?;
    assert!(error.contains("[velnor-gen]"), "{error}");
    assert!(error.contains("linux-x64"), "{error}");
    Ok(())
}

#[test]
fn a_tampered_version_is_refused_and_the_message_names_the_field() -> TestResult {
    let tampered = lock()?.replacen("version = \"0.1.81\"", "version = \"0.1.82\"", 1);
    let error = check(&tampered, release_binary()).err().ok_or("accepted")?;
    assert!(error.starts_with("version:"), "{error}");
    Ok(())
}

#[test]
fn a_tampered_commit_is_refused_and_the_message_names_the_field() -> TestResult {
    let tampered = lock()?.replacen(COMMIT, OTHER_COMMIT, 1);
    let error = check(&tampered, release_binary()).err().ok_or("accepted")?;
    assert!(error.starts_with("commit:"), "{error}");
    Ok(())
}

#[test]
fn online_a_lock_that_matches_the_release_digests_passes() -> TestResult {
    let text = lock()?;
    assert_eq!(
        verify_release(text.as_bytes(), SUMS, COMMIT_FILE, None).map_err(|e| e.to_string()),
        Ok(())
    );
    Ok(())
}

#[test]
fn online_a_tampered_generator_digest_is_refused_and_named() -> TestResult {
    let tampered = lock()?.replacen(GEN_LINUX_X64, &"0".repeat(64), 1);
    let error = verify_release(tampered.as_bytes(), SUMS, COMMIT_FILE, None)
        .err()
        .ok_or("accepted")?;
    let text = error.to_string();
    assert!(text.contains("[velnor-gen]"), "{text}");
    assert!(text.contains("linux-x64"), "{text}");
    Ok(())
}

#[test]
fn online_a_tampered_commit_is_refused_and_named() -> TestResult {
    let tampered = lock()?.replacen(COMMIT, OTHER_COMMIT, 1);
    let error = verify_release(tampered.as_bytes(), SUMS, COMMIT_FILE, None)
        .err()
        .ok_or("accepted")?;
    assert!(error.to_string().starts_with("commit:"), "{error}");
    Ok(())
}

/// `lock()` with an archive-format `[velnor-ci-binary]` section spliced in.
fn raw_lock_with_injected_binary_section() -> Result<String, Box<dyn Error>> {
    let archive = archive_lock_at("0.1.108")?;
    let section = archive
        .split("\n\n")
        .find(|block| block.starts_with("[velnor-ci-binary]"))
        .ok_or("the archive lock has no binary section")?;
    Ok(format!("{}\n\n{section}\n", lock()?.trim_end()))
}

#[test]
fn a_raw_lock_with_an_injected_binary_section_is_refused_and_the_digest_still_checked() -> TestResult
{
    let tampered =
        raw_lock_with_injected_binary_section()?.replacen(GEN_LINUX_X64, &"0".repeat(64), 1);
    let error = check(&tampered, release_binary()).err().ok_or("accepted")?;
    assert!(error.contains("[velnor-ci-binary]"), "{error}");
    assert!(error.contains("0.1.81"), "{error}");
    Ok(())
}

#[test]
fn an_archive_release_lock_without_a_binary_section_is_refused() -> TestResult {
    let archive = archive_lock_at("0.1.108")?;
    let stripped: Vec<&str> = archive
        .split("\n\n")
        .filter(|block| !block.starts_with("[velnor-ci-binary]"))
        .collect();
    let commit = archive
        .lines()
        .find_map(|line| line.strip_prefix("commit = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or("the lock names no commit")?;
    let running = RunningGenerator {
        version: "0.1.108",
        commit,
        platform: Some(Platform::LinuxX64),
        sha256: Some(GEN_LINUX_X64),
    };
    let error = check(&stripped.join("\n\n"), running)
        .err()
        .ok_or("accepted")?;
    assert!(error.contains("[velnor-ci-binary]"), "{error}");
    Ok(())
}

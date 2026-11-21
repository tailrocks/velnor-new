// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::Platform;
use velnor_rust::{
    LockError, ReleaseFiles, Remedy, RunningGenerator, check_generator_entry, check_lock_entries,
    pin_lock, verify_release,
};

use crate::lock::COMMIT_FILE;
use crate::lock_archive::{
    ARCHIVE_SUMS, BINARY_SUMS as CI_BINARY_SUMS, GENERATOR_BINARY_SUMS, generator_sums,
};

const BINARY_SUMS_FILE: &str = "BINARY-SHA256SUMS";

type TestResult = Result<(), Box<dyn Error>>;

const VERSION: &str = "0.1.111";
const COMMIT: &str = "1390e6f6a4b64e1f33dbddfbeedcd097db32ec36";
const GEN_ARCHIVE_X64: &str = "2a3ff18c1c5fc61a10aa66976ccf1fe669daa4af4eafec260b4c87d81afdf8f9";
const GEN_BINARY_X64: &str = "24da48018435c7e9b2e0153a05c7d65b68017db69b073cc8398c78dc6e4274df";
const GEN_BINARY_ARM64: &str = "8a7852af13610acee22317e764266301a83884595823a9b006a525d395cfa543";
const GEN_BINARY_MACOS: &str = "5b8f0d27e1a7f6a3c9d4e2b1a0c8f7e6d5c4b3a2918070605f4e3d2c1b0a9f8e";

fn lock() -> Result<String, Box<dyn Error>> {
    lock_at(VERSION)
}

fn lock_at(version: &str) -> Result<String, Box<dyn Error>> {
    Ok(pin_lock(
        version,
        COMMIT_FILE,
        &generator_sums(),
        Some(GENERATOR_BINARY_SUMS),
    )?)
}

fn release_files() -> ReleaseFiles {
    ReleaseFiles {
        sums: generator_sums(),
        commit: COMMIT_FILE.to_owned(),
        binary_sums: Some(GENERATOR_BINARY_SUMS.to_owned()),
    }
}

fn running(sha256: &str) -> RunningGenerator<'_> {
    RunningGenerator {
        version: VERSION,
        commit: COMMIT,
        platform: Some(Platform::LinuxX64),
        sha256: Some(sha256),
    }
}

fn check(text: &str, running: RunningGenerator<'_>) -> Result<(), LockError> {
    check_generator_entry(text.as_bytes(), running)
}

#[test]
fn the_lock_records_the_generator_archive_and_the_generator_binary_digests() -> TestResult {
    let lock = lock()?;
    for line in [
        format!("linux-x64 = \"{GEN_ARCHIVE_X64}\""),
        format!("linux-x64 = \"{GEN_BINARY_X64}\""),
        format!("linux-arm64 = \"{GEN_BINARY_ARM64}\""),
        format!("macos-arm64 = \"{GEN_BINARY_MACOS}\""),
    ] {
        assert!(lock.contains(&line), "{line} is missing from {lock}");
    }
    assert!(lock.contains("[velnor-gen-binary]"), "{lock}");
    Ok(())
}

#[test]
fn a_lock_pinned_from_the_release_passes_for_the_release_binary() -> TestResult {
    assert_eq!(check(&lock()?, running(GEN_BINARY_X64)), Ok(()));
    Ok(())
}

#[test]
fn the_archive_digest_is_not_the_digest_of_the_binary() -> TestResult {
    let error = check(&lock()?, running(GEN_ARCHIVE_X64))
        .err()
        .ok_or("accepted the archive digest as the binary's")?;
    assert!(error.to_string().contains("[velnor-gen-binary]"), "{error}");
    assert!(error.to_string().contains(GEN_BINARY_X64), "{error}");
    assert_eq!(
        error.remedy(),
        Some(&Remedy::Reinstall(VERSION.to_owned())),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_tampered_binary_digest_is_refused_online_and_named() -> TestResult {
    let tampered = lock()?.replacen(GEN_BINARY_X64, &"0".repeat(64), 1);
    let error = verify_release(
        tampered.as_bytes(),
        &generator_sums(),
        COMMIT_FILE,
        Some(GENERATOR_BINARY_SUMS),
    )
    .err()
    .ok_or("accepted")?;
    assert!(error.to_string().contains("[velnor-gen-binary]"), "{error}");
    assert!(error.to_string().contains("linux-x64"), "{error}");
    assert_eq!(
        error.remedy(),
        Some(&Remedy::Repin(VERSION.to_owned())),
        "{error}"
    );
    Ok(())
}

fn without_binary_section(lock: &str) -> String {
    lock.split("\n\n")
        .filter(|block| !block.starts_with("[velnor-gen-binary]"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[test]
fn a_lock_pinned_before_the_binary_section_still_passes() -> TestResult {
    let old = pin_lock(
        "0.1.110",
        COMMIT_FILE,
        &generator_sums(),
        Some(GENERATOR_BINARY_SUMS),
    )?;
    let older = RunningGenerator {
        version: "0.1.110",
        ..running(GEN_BINARY_X64)
    };
    assert_eq!(check(&old, older), Ok(()));
    Ok(())
}

#[test]
fn a_release_that_lists_binary_digests_is_refused_offline_without_its_section() -> TestResult {
    let error = check(&without_binary_section(&lock()?), running(GEN_BINARY_X64))
        .err()
        .ok_or("accepted a 0.1.111 lock without [velnor-gen-binary]")?;
    assert!(error.to_string().contains("[velnor-gen-binary]"), "{error}");
    assert_eq!(
        error.remedy(),
        Some(&Remedy::Repin(VERSION.to_owned())),
        "{error}"
    );
    Ok(())
}

#[test]
fn verify_release_names_the_repin_when_the_binary_section_is_missing() -> TestResult {
    let error = verify_release(
        without_binary_section(&lock()?).as_bytes(),
        &generator_sums(),
        COMMIT_FILE,
        Some(GENERATOR_BINARY_SUMS),
    )
    .err()
    .ok_or("accepted")?;
    assert!(error.to_string().contains("[velnor-gen-binary]"), "{error}");
    assert_eq!(
        error.remedy(),
        Some(&Remedy::Repin(VERSION.to_owned())),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_release_older_than_the_binary_section_is_pinned_without_it() -> TestResult {
    let lock = pin_lock(
        "0.1.110",
        COMMIT_FILE,
        &generator_sums(),
        Some(GENERATOR_BINARY_SUMS),
    )?;
    assert!(!lock.contains("velnor-gen-binary"), "{lock}");
    let injected = format!(
        "{}\n[velnor-gen-binary]\nlinux-arm64 = \"{GEN_BINARY_ARM64}\"\nlinux-x64 = \"{GEN_BINARY_X64}\"\n",
        lock.trim_end()
    );
    let older = RunningGenerator {
        version: "0.1.110",
        ..running(GEN_BINARY_X64)
    };
    let error = check(&injected, older).err().ok_or("accepted")?;
    assert!(error.to_string().contains("[velnor-gen-binary]"), "{error}");
    Ok(())
}

#[test]
fn a_release_from_the_binary_section_on_is_not_pinned_without_velnor_gen_digests() -> TestResult {
    let error = pin_lock(VERSION, COMMIT_FILE, ARCHIVE_SUMS, Some(CI_BINARY_SUMS))
        .err()
        .ok_or("pinned a 0.1.111 lock without the velnor-gen binary digests")?
        .to_string();
    assert!(error.contains("velnor-gen"), "{error}");
    assert!(error.contains(BINARY_SUMS_FILE), "{error}");
    Ok(())
}

#[test]
fn the_release_is_checked_before_the_offline_entry_so_a_tampered_binary_is_named_as_tampered()
-> TestResult {
    let tampered = lock()?.replacen(GEN_BINARY_X64, &"0".repeat(64), 1);
    let error = check_lock_entries(
        tampered.as_bytes(),
        Some(release_files()),
        running(GEN_BINARY_X64),
    )
    .err()
    .ok_or("accepted")?;
    assert_eq!(
        error.remedy(),
        Some(&Remedy::Repin(VERSION.to_owned())),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_repin_names_the_version_of_the_lock_it_repairs() -> TestResult {
    let later = "0.1.112";
    let tampered = lock_at(later)?.replacen(GEN_BINARY_X64, &"0".repeat(64), 1);
    let missing = without_binary_section(&lock_at(later)?);
    let cases = [
        ("online, tampered digest", tampered, Some(release_files())),
        ("offline, missing section", missing.clone(), None),
        ("online, missing section", missing, Some(release_files())),
    ];
    for (case, bytes, release) in cases {
        let error = check_lock_entries(bytes.as_bytes(), release, running(GEN_BINARY_X64))
            .err()
            .ok_or(case)?;
        assert_eq!(
            error.remedy(),
            Some(&Remedy::Repin(later.to_owned())),
            "{case}: {error}"
        );
    }
    Ok(())
}

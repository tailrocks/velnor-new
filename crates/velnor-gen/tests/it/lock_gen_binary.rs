// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::Platform;
use velnor_rust::{
    DetectError, Reason, RunningGenerator, check_generator_entry, pin_lock, verify_release,
};

use crate::lock::{COMMIT_FILE, with_lock};
use crate::lock_archive::ARCHIVE_SUMS;

type TestResult = Result<(), Box<dyn Error>>;

const VERSION: &str = "0.1.111";
const COMMIT: &str = "1390e6f6a4b64e1f33dbddfbeedcd097db32ec36";
const GEN_ARCHIVE_X64: &str = "2a3ff18c1c5fc61a10aa66976ccf1fe669daa4af4eafec260b4c87d81afdf8f9";
const GEN_BINARY_X64: &str = "24da48018435c7e9b2e0153a05c7d65b68017db69b073cc8398c78dc6e4274df";
const GEN_BINARY_ARM64: &str = "8a7852af13610acee22317e764266301a83884595823a9b006a525d395cfa543";
const GEN_BINARY_MACOS: &str = "5b8f0d27e1a7f6a3c9d4e2b1a0c8f7e6d5c4b3a2918070605f4e3d2c1b0a9f8e";
const BINARY_SUMS_DIGEST: &str = "69033b9cdde1860652f3d64fdb4f3757cafc0b666d50ad1ba409dda6b078ddaf";

/// `BINARY-SHA256SUMS` of a release that lists the velnor-gen binaries beside velnor-ci's.
const BINARY_SUMS: &str = "\
9de941ee8e31748973283b2cd31a439119a9d22f60ec3e2884d855a8e2971c6f  velnor-ci-x86_64-unknown-linux-musl.tar.gz/velnor-ci
6d887a0b48bba9c85da802b1943beb3ff38a2ffbc0effbdff615e5f53fe6a4a8  velnor-ci-aarch64-unknown-linux-musl.tar.gz/velnor-ci
a7e9a4897a81bb86338896573e43b79b4f05e34a489549f1687117dd456648ba  velnor-ci-aarch64-apple-darwin.tar.gz/velnor-ci
24da48018435c7e9b2e0153a05c7d65b68017db69b073cc8398c78dc6e4274df  velnor-gen-x86_64-unknown-linux-musl.tar.gz/velnor-gen
8a7852af13610acee22317e764266301a83884595823a9b006a525d395cfa543  velnor-gen-aarch64-unknown-linux-musl.tar.gz/velnor-gen
5b8f0d27e1a7f6a3c9d4e2b1a0c8f7e6d5c4b3a2918070605f4e3d2c1b0a9f8e  velnor-gen-aarch64-apple-darwin.tar.gz/velnor-gen
";

fn sums() -> String {
    ARCHIVE_SUMS.replace(
        "4e9a485939165d82a9ebd49078778ca47137f344524ce8d87d03116760d5bb6d",
        BINARY_SUMS_DIGEST,
    )
}

fn lock() -> Result<String, Box<dyn Error>> {
    Ok(pin_lock(VERSION, COMMIT_FILE, &sums(), Some(BINARY_SUMS))?)
}

fn running(sha256: &str) -> RunningGenerator<'_> {
    RunningGenerator {
        version: VERSION,
        commit: COMMIT,
        platform: Some(Platform::LinuxX64),
        sha256: Some(sha256),
    }
}

fn check(text: &str, running: RunningGenerator<'_>) -> Result<(), String> {
    check_generator_entry(text.as_bytes(), running).map_err(|error| error.to_string())
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
    assert!(error.contains("[velnor-gen-binary]"), "{error}");
    assert!(error.contains(GEN_BINARY_X64), "{error}");
    Ok(())
}

#[test]
fn a_tampered_binary_digest_is_refused_online_and_named() -> TestResult {
    let tampered = lock()?.replacen(GEN_BINARY_X64, &"0".repeat(64), 1);
    let error = verify_release(tampered.as_bytes(), &sums(), COMMIT_FILE, Some(BINARY_SUMS))
        .err()
        .ok_or("accepted")?
        .to_string();
    assert!(error.contains("[velnor-gen-binary]"), "{error}");
    assert!(error.contains("linux-x64"), "{error}");
    Ok(())
}

#[test]
fn a_lock_pinned_before_the_binary_section_still_passes() -> TestResult {
    let old: Vec<String> = lock()?
        .split("\n\n")
        .filter(|block| !block.starts_with("[velnor-gen-binary]"))
        .map(str::to_owned)
        .collect();
    assert_eq!(check(&old.join("\n\n"), running(GEN_BINARY_X64)), Ok(()));
    Ok(())
}

#[test]
fn a_release_older_than_the_binary_section_is_pinned_without_it() -> TestResult {
    let lock = pin_lock("0.1.110", COMMIT_FILE, &sums(), Some(BINARY_SUMS))?;
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
    assert!(error.contains("[velnor-gen-binary]"), "{error}");
    Ok(())
}

fn with_binary_table(table: &str) -> Result<String, Box<dyn Error>> {
    let pinned = lock()?;
    let head = pinned
        .split("[velnor-gen-binary]")
        .next()
        .ok_or("the lock has no binary table")?;
    Ok(format!("{head}[velnor-gen-binary]\n{table}\n"))
}

fn generation_refuses_the_lock(lock: &str) -> Result<bool, Box<dyn Error>> {
    Ok(matches!(
        expected_with_notes(&with_lock(lock)?).map(|(files, _)| files),
        Err(GenerateError::Detect(DetectError::Invalid {
            reason: Reason::HelperLock(_),
            ..
        }))
    ))
}

#[test]
fn generation_accepts_a_well_formed_generator_binary_table() -> TestResult {
    let sum = "b".repeat(64);
    let lock = with_binary_table(&format!("linux-x64 = \"{sum}\"\nlinux-arm64 = \"{sum}\""))?;
    assert!(!generation_refuses_the_lock(&lock)?, "{lock}");
    Ok(())
}

#[test]
fn generation_refuses_a_truncated_generator_binary_digest() -> TestResult {
    let lock = with_binary_table("linux-x64 = \"abc\"\nlinux-arm64 = \"abc\"")?;
    assert!(generation_refuses_the_lock(&lock)?, "{lock}");
    Ok(())
}

#[test]
fn generation_refuses_a_generator_binary_table_missing_a_required_platform() -> TestResult {
    let sum = "b".repeat(64);
    let lock = with_binary_table(&format!("linux-x64 = \"{sum}\""))?;
    assert!(generation_refuses_the_lock(&lock)?, "{lock}");
    Ok(())
}

#[test]
fn generation_refuses_a_generator_binary_table_with_an_unknown_platform() -> TestResult {
    let sum = "b".repeat(64);
    let lock = with_binary_table(&format!(
        "linux-x64 = \"{sum}\"\nlinux-arm64 = \"{sum}\"\nlinux-riscv64 = \"{sum}\""
    ))?;
    assert!(generation_refuses_the_lock(&lock)?, "{lock}");
    Ok(())
}

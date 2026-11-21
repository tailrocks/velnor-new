// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::Platform;
use velnor_rust::{
    DetectError, LOCK_PATH, Reason, RunningGenerator, check_generator_entry, detect, pin_lock,
    verify_release,
};

use crate::facts::{lib, repo};
use crate::lock::{COMMIT_FILE, with_lock};
use crate::lock_archive::{
    ARCHIVE_SUMS, BINARY_SUMS as CI_BINARY_SUMS, GENERATOR_BINARY_SUMS, generator_sums,
};

const BINARY_SUMS_FILE: &str = "BINARY-SHA256SUMS";

type TestResult = Result<(), Box<dyn Error>>;

const VERSION: &str = "0.1.111";
const COMMIT: &str = "1390e6f6a4b64e1f33dbddfbeedcd097db32ec36";
const CI_ARCHIVE_X64: &str = "b91a87ecac8eda2809f202b4566d886273d7b34d6ef29f726751fcbc8bb3d502";
const CI_BINARY_X64: &str = "9de941ee8e31748973283b2cd31a439119a9d22f60ec3e2884d855a8e2971c6f";
const GEN_ARCHIVE_X64: &str = "2a3ff18c1c5fc61a10aa66976ccf1fe669daa4af4eafec260b4c87d81afdf8f9";
const GEN_BINARY_X64: &str = "24da48018435c7e9b2e0153a05c7d65b68017db69b073cc8398c78dc6e4274df";
const GEN_BINARY_ARM64: &str = "8a7852af13610acee22317e764266301a83884595823a9b006a525d395cfa543";
const GEN_BINARY_MACOS: &str = "5b8f0d27e1a7f6a3c9d4e2b1a0c8f7e6d5c4b3a2918070605f4e3d2c1b0a9f8e";

fn lock() -> Result<String, Box<dyn Error>> {
    Ok(pin_lock(
        VERSION,
        COMMIT_FILE,
        &generator_sums(),
        Some(GENERATOR_BINARY_SUMS),
    )?)
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
    assert_eq!(
        error,
        format!(
            "[velnor-gen-binary] linux-x64 digest is {GEN_BINARY_X64} but this binary hashes to {GEN_ARCHIVE_X64}"
        )
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
    .ok_or("accepted")?
    .to_string();
    assert_eq!(
        error,
        format!(
            "[velnor-gen-binary] linux-x64 digest is {} but the release lists {GEN_BINARY_X64}",
            "0".repeat(64)
        )
    );
    Ok(())
}

fn lock_refused(text: &str) -> TestResult {
    let view = repo(
        vec![("Cargo.toml", vec![lib("a", "a/Cargo.toml")?])],
        &[(LOCK_PATH.as_str(), text)],
    )?;
    match detect(&view) {
        Err(DetectError::Invalid {
            path,
            reason: Reason::HelperLock(_),
        }) if path == LOCK_PATH => Ok(()),
        Err(error) => Err(format!("refused at another file or reason: {error:?}").into()),
        Ok(_) => Err("accepted".into()),
    }
}

#[test]
fn detect_refuses_a_malformed_digest_in_each_lock_table() -> TestResult {
    for (table, digest) in [
        ("velnor-gen-binary", GEN_BINARY_X64),
        ("velnor-gen", GEN_ARCHIVE_X64),
        ("velnor-ci", CI_ARCHIVE_X64),
        ("velnor-ci-binary", CI_BINARY_X64),
    ] {
        let text = lock()?.replacen(digest, "not-a-digest", 1);
        lock_refused(&text).map_err(|error| format!("[{table}] {error}"))?;
    }
    Ok(())
}

fn without_binary_section() -> Result<String, Box<dyn Error>> {
    Ok(lock()?
        .split("\n\n")
        .filter(|block| !block.starts_with("[velnor-gen-binary]"))
        .collect::<Vec<_>>()
        .join("\n\n"))
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
    let error = check(&without_binary_section()?, running(GEN_BINARY_X64))
        .err()
        .ok_or("accepted a 0.1.111 lock without [velnor-gen-binary]")?;
    assert_eq!(
        error,
        "[velnor-gen-binary] is missing: release 0.1.111 lists the velnor-gen binary digests, so the lock must list them; re-pin with `velnor-gen pin-helper 0.1.111` or later"
    );
    Ok(())
}

#[test]
fn verify_release_names_the_repin_when_the_binary_section_is_missing() -> TestResult {
    let error = verify_release(
        without_binary_section()?.as_bytes(),
        &generator_sums(),
        COMMIT_FILE,
        Some(GENERATOR_BINARY_SUMS),
    )
    .err()
    .ok_or("accepted")?
    .to_string();
    assert_eq!(
        error,
        "[velnor-gen-binary] is missing: release 0.1.111 lists the velnor-gen binary digests; re-pin with `velnor-gen pin-helper 0.1.111` or later"
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
    assert_eq!(
        error,
        "[velnor-gen-binary] is not allowed: release 0.1.110 does not read it"
    );
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

fn with_binary_table(table: &str) -> Result<String, Box<dyn Error>> {
    let pinned = lock()?;
    let head = pinned
        .split("[velnor-gen-binary]")
        .next()
        .ok_or("the lock has no binary table")?;
    Ok(format!("{head}[velnor-gen-binary]\n{table}\n"))
}

fn generation_refused(lock: &str) -> Result<String, Box<dyn Error>> {
    match expected_with_notes(&with_lock(lock)?) {
        Err(GenerateError::Detect(DetectError::Invalid {
            path,
            reason: Reason::HelperLock(error),
        })) if path == LOCK_PATH => Ok(error.to_string()),
        Err(error) => Err(format!("refused at another file or reason: {error}").into()),
        Ok(_) => Err("generation accepted the lock".into()),
    }
}

fn malformed_binary_tables() -> [String; 3] {
    let sum = "b".repeat(64);
    [
        "linux-x64 = \"abc\"\nlinux-arm64 = \"abc\"".to_string(),
        format!("linux-x64 = \"{sum}\""),
        format!("linux-x64 = \"{sum}\"\nlinux-arm64 = \"{sum}\"\nlinux-riscv64 = \"{sum}\""),
    ]
}

#[test]
fn generation_accepts_a_well_formed_generator_binary_table() -> TestResult {
    let sum = "b".repeat(64);
    let lock = with_binary_table(&format!("linux-x64 = \"{sum}\"\nlinux-arm64 = \"{sum}\""))?;
    expected_with_notes(&with_lock(&lock)?).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn generation_refuses_a_malformed_generator_binary_table() -> TestResult {
    for table in malformed_binary_tables() {
        generation_refused(&with_binary_table(&table)?)?;
    }
    Ok(())
}

#[test]
fn generation_names_the_generator_binary_table_in_its_refusal() -> TestResult {
    for table in malformed_binary_tables() {
        let message = generation_refused(&with_binary_table(&table)?)?;
        assert!(message.starts_with("[velnor-gen-binary] "), "{message}");
    }
    Ok(())
}

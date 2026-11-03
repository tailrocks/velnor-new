// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::tools_cache_install_run::{Install, Outcome, run_install};
use super::tools_cache_pin_fixture::{
    MACHETE_BINARY, MACHETE_DIR, MACHETE_VERSION, machete_archive, pins_with_fixture_sha,
};
use super::tools_cache_verify_hook::step_in;

const RESTORE_ACTION: &str = ".github/actions/velnor-tools-cache-restore/action.yml";
const REVERIFY_STEP: &str = "Re-verify restored Mise installs";
const LOCK_BINARY: &[u8] = b"repository lock cargo-machete binary\n";
const DIFFERS: &str = "a file differs from the pinned archive";

fn lock_row(version: &str, sha256: &str) -> String {
    format!(
        "[[tools.\"http:cargo-machete\"]]\nversion = \"{version}\"\n\n[tools.\"http:cargo-machete\".\"platforms.linux-x64\"]\nchecksum = \"sha256:{sha256}\"\nurl = \"https://example.invalid/cargo-machete-lock.tar.gz\"\n"
    )
}

fn outcome_for(
    lock_version: &str,
    install_version: &str,
    install_file: &[u8],
) -> Result<Outcome, Box<dyn Error>> {
    let step = step_in(RESTORE_ACTION, REVERIFY_STEP)?;
    let rendered = step
        .tool_pins
        .as_deref()
        .ok_or("restore step has no pins")?;
    let scratch_a = tempfile::tempdir()?;
    let scratch_b = tempfile::tempdir()?;
    let (archive_a, sha_a) = machete_archive(scratch_a.path(), MACHETE_BINARY)?;
    let (archive_b, sha_b) = machete_archive(scratch_b.path(), LOCK_BINARY)?;
    assert_ne!(sha_a, sha_b);
    let pins = pins_with_fixture_sha(rendered, &sha_a);
    let lock = lock_row(lock_version, &sha_b);
    run_install(
        &step,
        &Install {
            dir: MACHETE_DIR,
            version: install_version,
            pins: &pins,
            lock: Some(&lock),
            archive_sha256: &sha_a,
            archive: &archive_a,
            extra_archives: &[(&sha_b, &archive_b)],
            file: install_file,
        },
        &[],
    )
}

#[test]
fn lock_row_cannot_replace_a_catalog_pin_for_the_same_name_and_version()
-> Result<(), Box<dyn Error>> {
    let rejected = outcome_for(MACHETE_VERSION, MACHETE_VERSION, LOCK_BINARY)?;
    assert_eq!(
        (rejected.exit, rejected.kept),
        (Some(0), false),
        "stdout: {}",
        rejected.stdout
    );
    assert!(
        rejected.stdout.contains(DIFFERS),
        "stdout: {}",
        rejected.stdout
    );
    assert!(
        !rejected
            .stdout
            .contains("restored http-cargo-machete@0.9.2 matches"),
        "stdout: {}",
        rejected.stdout
    );
    Ok(())
}

#[test]
fn install_matching_the_catalog_pin_is_kept_despite_a_conflicting_lock_row()
-> Result<(), Box<dyn Error>> {
    let kept = outcome_for(MACHETE_VERSION, MACHETE_VERSION, MACHETE_BINARY)?;
    assert_eq!((kept.exit, kept.kept), (Some(0), true), "{}", kept.stdout);
    assert!(
        kept.stdout
            .contains("restored http-cargo-machete@0.9.2 matches its mise.lock archive"),
        "stdout: {}",
        kept.stdout
    );
    assert!(
        !kept.stdout.contains("discarding"),
        "stdout: {}",
        kept.stdout
    );
    Ok(())
}

#[test]
fn lock_row_pins_a_version_the_catalog_does_not() -> Result<(), Box<dyn Error>> {
    let kept = outcome_for("0.9.1", "0.9.1", LOCK_BINARY)?;
    assert_eq!((kept.exit, kept.kept), (Some(0), true), "{}", kept.stdout);
    assert!(
        kept.stdout
            .contains("restored http-cargo-machete@0.9.1 matches"),
        "stdout: {}",
        kept.stdout
    );
    Ok(())
}

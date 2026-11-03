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
const MATCHES: &str = "restored http-cargo-machete@0.9.2 matches its mise.lock archive";

fn outcome_with_link(link: &str, target: &str) -> Result<Outcome, Box<dyn Error>> {
    let step = step_in(RESTORE_ACTION, REVERIFY_STEP)?;
    let rendered = step
        .tool_pins
        .as_deref()
        .ok_or("restore step has no pins")?;
    let scratch = tempfile::tempdir()?;
    let (archive, sha256) = machete_archive(scratch.path(), MACHETE_BINARY)?;
    let pins = pins_with_fixture_sha(rendered, &sha256);
    run_install(
        &step,
        &Install {
            dir: MACHETE_DIR,
            version: MACHETE_VERSION,
            pins: &pins,
            lock: None,
            archive_sha256: &sha256,
            archive: &archive,
            extra_archives: &[],
            file: MACHETE_BINARY,
            links: &[(link, target)],
            workspace_links: &[],
            files: &[],
            home_prefix: "",
        },
        &[],
    )
}

fn assert_discarded(link: &str, target: &str) -> Result<(), Box<dyn Error>> {
    let outcome = outcome_with_link(link, target)?;
    assert_eq!(
        (outcome.exit, outcome.kept),
        (Some(0), false),
        "target {target}, stdout: {}",
        outcome.stdout
    );
    let reason = format!("symlink {link} leaves the install");
    assert!(
        outcome.stdout.contains(&reason),
        "target {target}, stdout: {}",
        outcome.stdout
    );
    assert!(
        !outcome.stdout.contains(MATCHES),
        "target {target}, stdout: {}",
        outcome.stdout
    );
    Ok(())
}

#[test]
fn install_link_that_climbs_out_through_its_own_directory_is_discarded()
-> Result<(), Box<dyn Error>> {
    assert_discarded(".mise-bins/x", "{dir}/../../../evil")
}

#[test]
fn install_links_pointing_outside_are_discarded() -> Result<(), Box<dyn Error>> {
    assert_discarded(".mise-bins/x", "/etc/hostname")?;
    assert_discarded(".mise-bins/x", "../../evil")
}

#[test]
fn mise_bins_link_to_a_file_inside_the_install_is_kept() -> Result<(), Box<dyn Error>> {
    let kept = outcome_with_link(".mise-bins/cargo-machete", "{dir}/file")?;
    assert_eq!((kept.exit, kept.kept), (Some(0), true), "{}", kept.stdout);
    assert!(kept.stdout.contains(MATCHES), "stdout: {}", kept.stdout);
    assert!(
        !kept.stdout.contains("discarding"),
        "stdout: {}",
        kept.stdout
    );
    Ok(())
}

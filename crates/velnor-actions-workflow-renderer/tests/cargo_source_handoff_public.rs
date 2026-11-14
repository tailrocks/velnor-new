// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::{error::Error, path::Path, process::Command};

#[test]
fn locked_cargo_sources_survive_private_handoff_and_corruption_recovery()
-> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("python3")
        .arg("-I")
        .arg(root.join(
            "crates/velnor-actions-workflow-renderer/tests/cargo_source_handoff_scenarios.py",
        ))
        .arg(&root)
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn locked_git_sources_survive_private_handoff_without_executing_cache_configuration()
-> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output =
        Command::new("python3")
            .arg("-I")
            .arg(root.join(
                "crates/velnor-actions-workflow-renderer/tests/cargo_git_handoff_scenarios.py",
            ))
            .arg(&root)
            .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn stable_qualification_reuses_both_locked_source_roots_on_a_fresh_consumer()
-> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("python3")
        .arg("-I")
        .arg(root.join(
            "crates/velnor-actions-workflow-renderer/tests/stable_source_handoff_scenarios.py",
        ))
        .arg(&root)
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn stable_qualification_refuses_artifacts_with_wrong_producer_identity()
-> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("python3")
        .arg("-I")
        .arg(root.join(
            "crates/velnor-actions-workflow-renderer/tests/stable_source_admission_scenarios.py",
        ))
        .arg(&root)
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

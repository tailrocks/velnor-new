// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use super::bounded::copy_oracle;
use super::support::{Harness, diagnostics_candidate};

#[test]
fn capture_creates_absent_oracle_and_check_accepts_it() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let oracle = harness.directory.join("new-oracle");
    let check = harness.run(
        "check",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;
    assert_eq!(check.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&check.stderr).contains("golden cases directory is missing"));
    assert!(!oracle.exists());
    let capture = harness.run(
        "capture",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;
    assert!(
        capture.status.success(),
        "{}",
        String::from_utf8_lossy(&capture.stderr)
    );
    let check = harness.run(
        "check",
        &harness.root,
        &candidate,
        &harness.archive_guard,
        &harness.guard_sha256,
        Some(&oracle),
    )?;
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("ALL GOLDENS MATCH"));
    Ok(())
}

#[test]
fn capture_rejects_candidate_created_destination() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let oracle = harness.directory.join("new-oracle");
    let output = harness
        .command(
            "capture",
            &harness.root,
            &candidate,
            &harness.archive_guard,
            &harness.guard_sha256,
            Some(&oracle),
        )
        .env("VELNOR_PLAN_DIAGNOSTICS_MODE", "create-absent-oracle")
        .env("VELNOR_PLAN_DIAGNOSTICS_ROOT", &harness.root)
        .env("VELNOR_PLAN_DIAGNOSTICS_MUTATION_ROOT", &harness.directory)
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("golden oracle changed during candidate execution")
    );
    assert_eq!(fs::read(oracle.join("sentinel"))?, b"candidate-created\n");
    assert!(!oracle.join("cases").exists());
    assert!(!oracle.join("MANIFEST.sha256").exists());
    Ok(())
}

#[test]
fn rejects_a_candidate_that_forges_the_oracle_to_match_its_output() -> Result<(), Box<dyn Error>> {
    let harness = Harness::new()?;
    let candidate = diagnostics_candidate()?;
    let source = harness.root.join("fixtures/opentofu-goldens");
    let original_case = fs::read(source.join("cases/nested/preview/.github/CLAUDE.md"))?;
    let original_manifest = fs::read(source.join("MANIFEST.sha256"))?;
    let oracle = harness.directory.join("forged-oracle");
    copy_oracle(&source, &oracle)?;
    let target = oracle.join("cases/nested/preview/.github/CLAUDE.md");
    let manifest = oracle.join("MANIFEST.sha256");
    let before_target = fs::read(&target)?;
    let before_manifest = fs::read(&manifest)?;
    let output = harness
        .command(
            "check",
            &harness.root,
            &candidate,
            &harness.archive_guard,
            &harness.guard_sha256,
            Some(&oracle),
        )
        .env(
            "VELNOR_PLAN_DIAGNOSTICS_MODE",
            "forge-oracle-after-generate",
        )
        .env("VELNOR_PLAN_DIAGNOSTICS_ROOT", &harness.root)
        .env("VELNOR_PLAN_DIAGNOSTICS_MUTATION_ROOT", &harness.directory)
        .output()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("golden oracle changed during candidate execution"),
        "unexpected diagnostic: {stderr}"
    );
    assert_ne!(fs::read(&target)?, before_target);
    assert_ne!(fs::read(&manifest)?, before_manifest);
    assert_eq!(
        fs::read(source.join("cases/nested/preview/.github/CLAUDE.md"))?,
        original_case
    );
    assert_eq!(fs::read(source.join("MANIFEST.sha256"))?, original_manifest);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("ALL GOLDENS MATCH"));
    Ok(())
}

#[test]
fn capture_rejects_changes_to_unmanaged_oracle_files() -> Result<(), Box<dyn Error>> {
    for mode in ["modify-oracle-extra", "create-oracle-extra"] {
        let harness = Harness::new()?;
        let candidate = diagnostics_candidate()?;
        let oracle = harness.directory.join("oracle");
        copy_oracle(&harness.root.join("fixtures/opentofu-goldens"), &oracle)?;
        fs::write(oracle.join("README.md"), b"original\n")?;
        let manifest = fs::read(oracle.join("MANIFEST.sha256"))?;
        let output = harness
            .command(
                "capture",
                &harness.root,
                &candidate,
                &harness.archive_guard,
                &harness.guard_sha256,
                Some(&oracle),
            )
            .env("VELNOR_PLAN_DIAGNOSTICS_MODE", mode)
            .env("VELNOR_PLAN_DIAGNOSTICS_ROOT", &harness.root)
            .env("VELNOR_PLAN_DIAGNOSTICS_MUTATION_ROOT", &harness.directory)
            .output()?;
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("golden oracle changed during candidate execution")
        );
        assert_eq!(fs::read(oracle.join("MANIFEST.sha256"))?, manifest);
    }
    Ok(())
}

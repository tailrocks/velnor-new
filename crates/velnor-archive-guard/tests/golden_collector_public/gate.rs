// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, process::Command};

use super::executable_custody::add_fixture_origin;
use super::mutation::copy_mutation_repository;
use super::support::{Harness, copy_executable, copy_tree, diagnostics_candidate};

#[test]
fn repository_golden_gate_accepts_matching_and_rejects_changed_oracle() -> Result<(), Box<dyn Error>>
{
    let harness = Harness::new()?;
    let root = harness.directory.join("source");
    copy_mutation_repository(&harness.root, &root)?;
    add_fixture_origin(&root)?;
    let oracle = root.join("fixtures/opentofu-goldens");
    copy_tree(&harness.root.join("fixtures/opentofu-goldens"), &oracle)?;
    let target = harness.directory.join("target");
    fs::create_dir_all(target.join("release"))?;
    copy_executable(
        &diagnostics_candidate()?,
        &target.join("release/velnor-actions"),
    )?;
    copy_executable(
        &harness.collector,
        &target.join("release/velnor-golden-collector"),
    )?;
    let runner_temp = harness.directory.join("runner");
    fs::create_dir_all(runner_temp.join("source-qualification-artifacts/logs"))?;
    fs::create_dir_all(runner_temp.join("velnor-source-qualification-tmp"))?;
    let invoke = || -> Result<std::process::Output, Box<dyn Error>> {
        Ok(Command::new("bash")
            .arg(root.join("scripts/generator-release/source-qualification.sh"))
            .arg("check-source-output-against-checked-in-repository-goldens")
            .current_dir(&root)
            .env("RUNNER_TEMP", &runner_temp)
            .env("CARGO_TARGET_DIR", &target)
            .env("VELNOR_QUALIFICATION_ARCHIVE_GUARD", &harness.archive_guard)
            .env(
                "VELNOR_QUALIFICATION_ARCHIVE_GUARD_SHA256",
                &harness.guard_sha256,
            )
            .output()?)
    };
    let success = invoke()?;
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    let log = runner_temp.join("source-qualification-artifacts/logs/repository-oracle-check.log");
    assert!(fs::read_to_string(&log)?.contains("ALL GOLDENS MATCH"));
    fs::write(oracle.join("cases/nested/plan.txt"), b"changed golden\n")?;
    let failure = invoke()?;
    assert!(!failure.status.success());
    assert!(fs::read_to_string(&log)?.contains("golden manifest"));
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path, process::Command};

use super::executable_custody::add_fixture_origin;
use super::mutation::copy_mutation_repository;
use super::support::{
    Harness, copy_executable, copy_tree, diagnostics_candidate, dogfood_plan_difference,
};

#[test]
fn repository_golden_gate_accepts_matching_and_rejects_changed_oracle() -> Result<(), Box<dyn Error>>
{
    let harness = Harness::new()?;
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
    let evidence = runner_temp.join("source-qualification-artifacts");
    fs::create_dir_all(evidence.join("logs"))?;
    fs::create_dir_all(runner_temp.join("velnor-source-qualification-tmp"))?;
    let invoke = |root: &Path| -> Result<std::process::Output, Box<dyn Error>> {
        Ok(Command::new("bash")
            .arg(root.join("scripts/generator-release/source-qualification.sh"))
            .arg("check-source-output-against-checked-in-repository-goldens")
            .current_dir(root)
            .env("RUNNER_TEMP", &runner_temp)
            .env("CARGO_TARGET_DIR", &target)
            .env("VELNOR_QUALIFICATION_ARCHIVE_GUARD", &harness.archive_guard)
            .env(
                "VELNOR_QUALIFICATION_ARCHIVE_GUARD_SHA256",
                &harness.guard_sha256,
            )
            .output()?)
    };
    let log = evidence.join("logs/repository-oracle-check.log");
    let success = invoke(&harness.root)?;
    if !success.status.success() {
        let difference =
            dogfood_plan_difference(&evidence.join("repository-oracle-stage"), &harness.root)?;
        return Err(format!(
            "repository golden gate failed: {}; dogfood plan {difference}",
            fs::read_to_string(&log)?
        )
        .into());
    }
    let checked = fs::read_to_string(&log)?;
    for case in [
        "nested",
        "mbx-nextest",
        "empty-suite",
        "minimal-cargo",
        "dogfood",
    ] {
        assert!(
            checked.contains(&format!("match: {case}")),
            "missing match for {case}"
        );
    }
    assert!(checked.contains("ALL GOLDENS MATCH"));

    let root = harness.directory.join("source");
    copy_mutation_repository(&harness.root, &root)?;
    add_fixture_origin(&root)?;
    let oracle = root.join("fixtures/opentofu-goldens");
    copy_tree(&harness.root.join("fixtures/opentofu-goldens"), &oracle)?;
    fs::write(oracle.join("cases/nested/plan.txt"), b"changed golden\n")?;
    let failure = invoke(&root)?;
    assert!(!failure.status.success());
    assert!(fs::read_to_string(&log)?.contains("golden manifest"));
    Ok(())
}

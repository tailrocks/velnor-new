// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;

use velnor_actions_contract::workflow::UnitOutcome;
use velnor_actions_contract::{JobDisposition, TaskExecutionManifestV1};

use super::tofu_generated_ledger_fixture::{Fixture, TestResult};

fn verify_generated_groups(mixed: bool) -> TestResult {
    let fixture = Fixture::new(mixed)?;
    let mut selected = BTreeSet::new();
    for job in &fixture.plan.jobs {
        if job.disposition != JobDisposition::Run {
            continue;
        }
        let expected = fixture.expected(&job.job_id);
        if expected.is_empty() {
            continue;
        }
        let ledger = fixture.emit(&job.job_id)?;
        assert_eq!(
            ledger
                .selected_unit_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected
        );
        assert!(
            ledger
                .ordered_results
                .iter()
                .all(|unit| unit.outcome == UnitOutcome::Executed && unit.exit_code == Some(0))
        );
        selected.extend(ledger.selected_unit_ids);
    }
    assert!(selected.iter().any(|id| id.starts_with("stack/tofu/")));
    assert_eq!(
        selected.iter().any(|id| id.starts_with("stack/rust/")),
        mixed
    );
    Ok(())
}

#[test]
fn pure_tofu_generation_admits_every_reported_root_obligation() -> TestResult {
    verify_generated_groups(false)
}

#[test]
fn mixed_generation_admits_both_rust_and_tofu_reported_obligations() -> TestResult {
    verify_generated_groups(true)
}

#[test]
fn generated_tofu_manifest_rejects_missing_or_corrupted_records() -> TestResult {
    for corruption in ["missing", "entry", "digest", "payload", "environment"] {
        let fixture = Fixture::new(false)?;
        let path = fixture.manifest_path();
        let text = fs::read_to_string(&path)?;
        let mut manifest: TaskExecutionManifestV1 =
            serde_json::from_str(text.split_once('\n').ok_or("manifest marker missing")?.1)?;
        match corruption {
            "missing" => fs::remove_file(&path)?,
            "entry" => {
                let first = manifest.tasks.keys().next().ok_or("task missing")?.clone();
                manifest.tasks.remove(&first);
                fs::write(&path, manifest.marked_json()?)?;
            }
            mode => {
                let record = manifest.tasks.values_mut().next().ok_or("task missing")?;
                match mode {
                    "digest" => record.execution_digest = format!("b3-{}", "0".repeat(64)),
                    "payload" => {
                        record.argv.push("-destroy".to_owned());
                        record.task_digest = velnor_actions_contract::workflow::crate_job::task_digest_for_execution(&record.task_id, &record.argv, &velnor_actions_contract::cachekey::toolchain_id(&record.toolchain_inputs)?)?;
                        record.refresh_execution_digest()?;
                    }
                    "environment" => {
                        record
                            .env
                            .insert("TF_TOKEN_example".to_owned(), "secret-shaped".to_owned());
                        record.refresh_execution_digest()?;
                    }
                    _ => return Err("unknown corruption".into()),
                }
                fs::write(
                    &path,
                    format!(
                        "{}\n{}\n",
                        manifest.marker_line()?,
                        serde_json::to_string(&manifest)?
                    ),
                )?;
            }
        }
        let job = fixture
            .plan
            .jobs
            .iter()
            .find(|job| !fixture.expected(&job.job_id).is_empty())
            .ok_or("job missing")?;
        let error = fixture
            .emit(&job.job_id)
            .expect_err("accepted corrupted native manifest")
            .to_string();
        let expected = match corruption {
            "missing" => "missing_manifest",
            "entry" => "task_without_manifest_entry",
            "digest" => "digest_mismatch",
            "payload" => "invalid_tofu_payload",
            "environment" => "invalid_tofu_environment",
            _ => return Err("unknown corruption".into()),
        };
        assert!(error.contains(expected), "{corruption}: {error}");
    }
    Ok(())
}

#[test]
fn generated_tofu_manifest_binds_recipe_to_selected_plan() -> TestResult {
    let fixture = Fixture::new(false)?;
    let path = fixture.manifest_path();
    let text = fs::read_to_string(&path)?;
    let mut manifest: TaskExecutionManifestV1 =
        serde_json::from_str(text.split_once('\n').ok_or("manifest marker missing")?.1)?;
    let record = manifest.tasks.values_mut().next().ok_or("task missing")?;
    let selector = "opentofu@1.13.7".to_owned();
    record.argv[5] = selector.clone();
    record.toolchain_inputs.tools = vec![selector.clone()];
    let parts: Vec<_> = record.task_id.split('/').collect();
    record.toolchain_inputs.components =
        velnor_actions_tofu::provider_toolchain_entries(parts[2], parts[3], &selector)?;
    record.task_digest = velnor_actions_contract::workflow::crate_job::task_digest_for_execution(
        &record.task_id,
        &record.argv,
        &velnor_actions_contract::cachekey::toolchain_id(&record.toolchain_inputs)?,
    )?;
    record.refresh_execution_digest()?;
    manifest.validate()?;
    fs::write(path, manifest.marked_json()?)?;
    let job = fixture
        .plan
        .jobs
        .iter()
        .find(|job| !fixture.expected(&job.job_id).is_empty())
        .ok_or("job missing")?;
    let error = fixture
        .emit(&job.job_id)
        .expect_err("accepted different valid recipe")
        .to_string();
    assert!(error.contains("manifest_task_digest_mismatch"), "{error}");
    Ok(())
}

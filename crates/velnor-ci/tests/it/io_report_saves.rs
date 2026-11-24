// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use serde_json::json;
use std::fs;
use std::process::Command;
use velnor_ci::report::{LaneMetrics, SaveOutcome};

type ResultTest = Result<(), Box<dyn std::error::Error>>;

#[test]
fn report_observes_a_failed_save_step_of_a_cache_descriptor() -> ResultTest {
    let repo = Repo::new("report-failed-save");
    let plan =
        json!([{"family":"sources","root":null,"restore_step":"restore","save_step":"save"}]);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("GITHUB_REF", "refs/heads/main")
        .env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
        .env("VELNOR_REPORT_CACHES", plan.to_string())
        .env(
            "VELNOR_REPORT_STEPS",
            r#"{"restore":{"outcome":"success","outputs":{}},"save":{"outcome":"failure","outputs":{}}}"#,
        )
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(metrics(&repo).caches[0].save, SaveOutcome::Failed);
    Ok(())
}

#[test]
fn report_without_a_save_step_keeps_schema_1() -> ResultTest {
    let repo = Repo::new("report-no-save-step");
    let plan = json!([{"family":"tools","root":null,"restore_step":"restore"}]);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "policy")
        .env("GITHUB_REF", "refs/heads/main")
        .env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
        .env("VELNOR_REPORT_CACHES", plan.to_string())
        .env(
            "VELNOR_REPORT_STEPS",
            r#"{"restore":{"outcome":"success","outputs":{}}}"#,
        )
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let metrics = metrics(&repo);
    assert_eq!(metrics.schema, 1);
    assert!(metrics.caches[0].save_step.is_none());
    Ok(())
}

fn metrics(repo: &Repo) -> LaneMetrics {
    serde_json::from_slice(
        &fs::read(repo.root().join("ci-metrics.json"))
            .unwrap_or_else(|error| unreachable!("{error}")),
    )
    .unwrap_or_else(|error| unreachable!("{error}"))
}

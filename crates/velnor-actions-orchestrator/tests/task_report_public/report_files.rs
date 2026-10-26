// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    CacheLayer, CacheResult, FinalReport, NEEDS_EXPECTED_ENV, TaskReport, canonical_json_bytes,
};
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context};

use super::TestResult;
use super::fixture::{RUN_ATTEMPT, RUN_ID, Scenario};
use super::process;

pub(super) fn write_task(
    scenario: &Scenario,
    test_name: &str,
    label: &str,
    task_id: &str,
    exit_code: i32,
    noop_digest: Option<&str>,
) -> TestResult<usize> {
    let result_path = scenario.root().join(format!("{label}.result"));
    let mut variables = vec![
        (OsString::from("GITHUB_RUN_ID"), OsString::from(RUN_ID)),
        (
            OsString::from("GITHUB_RUN_ATTEMPT"),
            OsString::from(RUN_ATTEMPT),
        ),
        (
            OsString::from("RUNNER_TEMP"),
            scenario.root().as_os_str().to_owned(),
        ),
        (OsString::from("VELNOR_TASK_ID"), OsString::from(task_id)),
        (
            OsString::from("VELNOR_EXIT_CODE"),
            OsString::from(exit_code.to_string()),
        ),
        (
            OsString::from("VELNOR_TASK_REPORT_CHILD_RESULT"),
            result_path.clone().into_os_string(),
        ),
    ];
    if let Some(digest) = noop_digest {
        variables.push((
            OsString::from("VELNOR_NOT_SELECTED_REASON"),
            OsString::from("not_in_plan"),
        ));
        variables.push((
            OsString::from("VELNOR_NOOP_TASK_DIGEST"),
            OsString::from(digest),
        ));
    }
    process::run_child(test_name, "write", label, scenario.root(), &variables)?;
    Ok(fs::read_to_string(result_path)?.parse()?)
}

pub(super) fn assemble(
    scenario: &Scenario,
    test_name: &str,
    label: &str,
    job_results: &[(&str, &str)],
) -> TestResult<String> {
    let mut needs = BTreeMap::from([("plan".to_owned(), "success".to_owned())]);
    for (job_id, conclusion) in job_results {
        needs.insert((*job_id).to_owned(), (*conclusion).to_owned());
    }
    let expected: Vec<String> = needs.keys().cloned().collect();
    let needs_json = serde_json::to_string(&needs)?;
    let expected_json = serde_json::to_string(&expected)?;
    let request_path = scenario.root().join(format!("{label}.request.json"));
    let variables = vec![
        (
            OsString::from("RUNNER_TEMP"),
            scenario.root().as_os_str().to_owned(),
        ),
        (
            OsString::from("VELNOR_TASK_REPORT_RUN_KEY"),
            OsString::from(&scenario.run_key),
        ),
        (
            OsString::from("VELNOR_TASK_REPORT_RUN_DIR"),
            scenario.run_dir.as_os_str().to_owned(),
        ),
        (
            OsString::from("VELNOR_TASK_REPORT_REQUEST"),
            request_path.clone().into_os_string(),
        ),
        (
            OsString::from("VELNOR_NEEDS_JSON"),
            OsString::from(needs_json),
        ),
        (
            OsString::from(NEEDS_EXPECTED_ENV),
            OsString::from(expected_json),
        ),
        (
            OsString::from("GITHUB_EVENT_NAME"),
            OsString::from("pull_request"),
        ),
        (
            OsString::from("GITHUB_EVENT_PATH"),
            scenario.event_path.as_os_str().to_owned(),
        ),
    ];
    process::run_child(test_name, "assemble", label, scenario.root(), &variables)?;
    Ok(fs::read_to_string(request_path)?)
}

pub(super) fn merge(request: &str) -> TestResult<FinalReport> {
    let merged = merge_internal_with_context(request, &MergeContext::detached())?;
    Ok(serde_json::from_str(&merged)?)
}

pub(super) fn task_reports(request: &str) -> TestResult<Vec<TaskReport>> {
    let value: serde_json::Value = serde_json::from_str(request)?;
    let reports = value
        .get("task_reports")
        .cloned()
        .ok_or_else(|| io::Error::other("task reports missing from merge request"))?;
    Ok(serde_json::from_value(reports)?)
}

pub(super) fn stage_all(scenario: &Scenario) -> TestResult<()> {
    for entry in &scenario.plan.matrix.include {
        let source = scenario
            .root()
            .join("velnor")
            .join(&scenario.run_key)
            .join(&entry.matrix_key);
        let target = scenario
            .run_dir
            .join("reports")
            .join(&entry.artifact_id)
            .join(&entry.matrix_key);
        fs::create_dir_all(target.join("tasks"))?;
        fs::copy(
            source.join("matrix-report.json"),
            target.join("matrix-report.json"),
        )?;
        copy_task_files(&source.join("tasks"), &target.join("tasks"))?;
    }
    Ok(())
}

pub(super) fn stage_provider_hit(scenario: &Scenario) -> TestResult<()> {
    assert_eq!(scenario.plan.matrix.include.len(), 1);
    let entry = &scenario.plan.matrix.include[0];
    let target = scenario
        .run_dir
        .join("reports")
        .join(&entry.artifact_id)
        .join(&entry.matrix_key);
    let matrix_path = target.join("matrix-report.json");
    let original_matrix = fs::read(&matrix_path)?;
    let task_path = only_task_path(&target.join("tasks"))?;
    let mut task: TaskReport = serde_json::from_slice(&fs::read(&task_path)?)?;
    task.cache.layer = CacheLayer::TofuProviders;
    task.cache.result = CacheResult::Hit;
    task.validate()?;
    fs::write(task_path, canonical_json_bytes(&task)?)?;
    assert_eq!(fs::read(matrix_path)?, original_matrix);
    Ok(())
}

fn copy_task_files(source: &Path, target: &Path) -> TestResult<()> {
    for item in fs::read_dir(source)? {
        let path = item?.path();
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("task filename missing"))?;
        fs::copy(&path, target.join(name))?;
    }
    Ok(())
}

fn only_task_path(directory: &Path) -> TestResult<PathBuf> {
    let paths: Vec<PathBuf> = fs::read_dir(directory)?
        .map(|item| item.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    assert_eq!(paths.len(), 1);
    paths
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::other("one task report expected").into())
}

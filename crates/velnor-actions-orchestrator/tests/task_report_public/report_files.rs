// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
#[cfg(unix)]
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use velnor_actions_contract::{
    CacheLayer, CacheResult, FinalReport, NEEDS_EXPECTED_ENV, TaskReport, canonical_json_bytes,
};
#[cfg(unix)]
use velnor_actions_mise::GH_VERSION;
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

#[cfg(unix)]
pub(super) fn stage_artifact_payload(scenario: &Scenario, payload: &Path) -> TestResult<()> {
    for entry in &scenario.plan.matrix.include {
        let source = scenario
            .root()
            .join("velnor")
            .join(&scenario.run_key)
            .join(&entry.matrix_key);
        let target = payload.join(&entry.matrix_key);
        fs::create_dir_all(target.join("tasks"))?;
        fs::copy(
            source.join("matrix-report.json"),
            target.join("matrix-report.json"),
        )?;
        copy_task_files(&source.join("tasks"), &target.join("tasks"))?;
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn install_fake_mise(scenario: &Scenario) -> TestResult<PathBuf> {
    let directory = scenario.root().join("fake-tool-bin");
    fs::create_dir_all(&directory)?;
    let executable = directory.join("mise");
    let script = format!(
        "#!/bin/sh\n\
set -eu\n\
[ \"$#\" -eq 16 ] || exit 80\n\
[ \"$1\" = --no-config ] || exit 81\n\
[ \"$2\" = --no-env ] || exit 82\n\
[ \"$3\" = --no-hooks ] || exit 83\n\
[ \"$4\" = exec ] || exit 84\n\
[ \"$5\" = gh@{GH_VERSION} ] || exit 85\n\
[ \"$6\" = -- ] || exit 86\n\
[ \"$7\" = gh ] || exit 87\n\
[ \"$MISE_NO_CONFIG\" = 1 ] || exit 88\n\
[ \"$MISE_NO_ENV\" = 1 ] || exit 89\n\
[ \"$MISE_NO_HOOKS\" = 1 ] || exit 90\n\
[ \"$MISE_LOCKFILE\" = 0 ] || exit 91\n\
[ \"$MISE_AUTO_INSTALL\" = false ] || exit 92\n\
[ \"$MISE_EXEC_AUTO_INSTALL\" = false ] || exit 93\n\
[ \"$GITHUB_REPOSITORY\" = tailrocks/velnor-new ] || exit 94\n\
shift 7\n\
[ \"$#\" -eq 9 ] || exit 95\n\
[ \"$1\" = run ] || exit 96\n\
[ \"$2\" = download ] || exit 97\n\
[ \"$3\" = \"$GITHUB_RUN_ID\" ] || exit 98\n\
[ \"$4\" = --name ] || exit 99\n\
[ \"$6\" = --dir ] || exit 100\n\
[ \"$8\" = --repo ] || exit 101\n\
[ \"$9\" = \"$GITHUB_REPOSITORY\" ] || exit 102\n\
exec \"$MISE_DATA_DIR/installs/gh/{GH_VERSION}/bin/gh\" \"$@\"\n"
    );
    fs::write(&executable, script)?;
    let mut permissions = fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions)?;
    Ok(directory)
}

#[cfg(unix)]
pub(super) fn retrieve(
    scenario: &Scenario,
    test_name: &str,
    label: &str,
    mise_data_dir: &Path,
    mise_bin_dir: &Path,
    artifact_payload: &Path,
    request_log: &Path,
) -> TestResult<usize> {
    let result_path = scenario.root().join(format!("{label}.result"));
    let inherited_path = env::var_os("PATH").ok_or_else(|| io::Error::other("PATH missing"))?;
    let path = env::join_paths(
        std::iter::once(mise_bin_dir.to_path_buf()).chain(env::split_paths(&inherited_path)),
    )?;
    let home = env::var_os("HOME").ok_or_else(|| io::Error::other("HOME missing"))?;
    let variables = vec![
        (OsString::from("GITHUB_RUN_ID"), OsString::from(RUN_ID)),
        (
            OsString::from("GITHUB_RUN_ATTEMPT"),
            OsString::from(RUN_ATTEMPT),
        ),
        (
            OsString::from("GITHUB_REPOSITORY"),
            OsString::from("tailrocks/velnor-new"),
        ),
        (
            OsString::from("RUNNER_TEMP"),
            scenario.root().as_os_str().to_owned(),
        ),
        (OsString::from("PATH"), path),
        (OsString::from("HOME"), home),
        (
            OsString::from("MISE_DATA_DIR"),
            mise_data_dir.as_os_str().to_owned(),
        ),
        (
            OsString::from("VELNOR_TEST_ARTIFACT_PAYLOAD"),
            artifact_payload.as_os_str().to_owned(),
        ),
        (
            OsString::from("VELNOR_TEST_GH_LOG"),
            request_log.as_os_str().to_owned(),
        ),
        (
            OsString::from("VELNOR_TASK_REPORT_CHILD_RESULT"),
            result_path.clone().into_os_string(),
        ),
    ];
    process::run_child(test_name, "retrieve", label, scenario.root(), &variables)?;
    Ok(fs::read_to_string(result_path)?.parse()?)
}

pub(super) fn stage_provider_hit(scenario: &Scenario, task_id: &str, key: &str) -> TestResult<()> {
    let entry = scenario
        .plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.task_id == task_id)
        .ok_or_else(|| io::Error::other("provider task matrix entry missing"))?;
    let target = scenario
        .run_dir
        .join("reports")
        .join(&entry.artifact_id)
        .join(&entry.matrix_key);
    let matrix_path = target.join("matrix-report.json");
    let original_matrix = fs::read(&matrix_path)?;
    let task_path = only_task_path(&target.join("tasks"))?;
    let mut task: TaskReport = serde_json::from_slice(&fs::read(&task_path)?)?;
    assert_eq!(task.task_id, task_id);
    task.cache.layer = CacheLayer::TofuProviders;
    task.cache.result = CacheResult::Hit;
    key.clone_into(&mut task.cache.key);
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

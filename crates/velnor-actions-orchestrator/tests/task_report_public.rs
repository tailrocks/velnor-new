// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "task_report_public/fixture.rs"]
mod fixture;
#[path = "task_report_public/process.rs"]
mod process;
#[path = "task_report_public/report_files.rs"]
mod report_files;

use velnor_actions_contract::{FinalStatus, NotSelectedReason, TaskStatus};

use std::io;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const MISSING_REPORTS_TEST: &str = "reports_move_not_run_to_passed_through_public_writer";
const BUILD_FAILURE_TEST: &str = "build_failure_generates_blocked_downstream_reports";
const NOOP_TEST: &str = "valid_selected_noop_reports_block_tasks";
const SHARED_ARTIFACT_TEST: &str = "shared_job_artifact_is_downloaded_once";
const PROVIDER_HIT_TEST: &str = "provider_hit_does_not_reclassify_executed_task";

#[test]
fn reports_move_not_run_to_passed_through_public_writer() -> TestResult<()> {
    if process::child_mode(MISSING_REPORTS_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[("clippy", "crate_clippy"), ("test", "crate_test")])?;
    let jobs = [("crate_clippy", "success"), ("crate_test", "success")];
    let missing = report_files::assemble(&scenario, MISSING_REPORTS_TEST, "missing", &jobs)?;
    let before = report_files::merge(&missing)?;
    assert_eq!(before.status, FinalStatus::PlanningFailed);
    assert_eq!(before.counts.executed, 0);
    assert!(before.counts.not_run > 0);

    for (index, task_id) in scenario.plan.task_ids.iter().enumerate() {
        assert_eq!(
            report_files::write_task(
                &scenario,
                MISSING_REPORTS_TEST,
                &format!("writer-{index}"),
                task_id,
                0,
                None,
            )?,
            1
        );
    }
    report_files::stage_all(&scenario)?;
    let complete = report_files::assemble(&scenario, MISSING_REPORTS_TEST, "complete", &jobs)?;
    let tasks = report_files::task_reports(&complete)?;
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|task| task.status == TaskStatus::Executed));
    let after = report_files::merge(&complete)?;
    let mut expected_artifacts: Vec<String> = scenario
        .plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.artifact_id.clone())
        .collect();
    expected_artifacts.sort();
    assert_eq!(after.status, FinalStatus::Passed);
    assert_eq!(after.counts.executed, 2);
    assert_eq!(after.counts.not_run, 0);
    assert_eq!(expected_artifacts.len(), 2);
    assert_ne!(expected_artifacts[0], expected_artifacts[1]);
    assert_eq!(after.downloaded_artifact_ids, expected_artifacts);
    Ok(())
}

#[test]
fn build_failure_generates_blocked_downstream_reports() -> TestResult<()> {
    if process::child_mode(BUILD_FAILURE_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[
        ("build", "crate_rust"),
        ("clippy", "crate_rust"),
        ("test", "crate_rust"),
    ])?;
    let build_id = fixture::task_id("build")?;
    assert_eq!(
        report_files::write_task(&scenario, BUILD_FAILURE_TEST, "build", &build_id, 1, None)?,
        3
    );
    report_files::stage_all(&scenario)?;
    let jobs = [("crate_rust", "failure")];
    let request = report_files::assemble(&scenario, BUILD_FAILURE_TEST, "failed-build", &jobs)?;
    let tasks = report_files::task_reports(&request)?;
    assert_eq!(tasks.len(), 3);
    let build = tasks
        .iter()
        .find(|task| task.task_id == build_id)
        .ok_or_else(|| io::Error::other("build report missing"))?;
    assert_eq!(build.status, TaskStatus::Failed);
    for kind in ["clippy", "test"] {
        let task_id = fixture::task_id(kind)?;
        let report = tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .ok_or_else(|| io::Error::other("downstream report missing"))?;
        assert_eq!(report.status, TaskStatus::NotSelected);
        assert_eq!(
            report.not_selected_reason,
            Some(NotSelectedReason::UpstreamFailed)
        );
    }
    let final_report = report_files::merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Failed);
    assert_eq!(final_report.counts.failed, 1);
    assert_eq!(final_report.counts.blocked, 2);
    assert_eq!(final_report.counts.not_run, 0);
    let artifact_id = &scenario.plan.matrix.include[0].artifact_id;
    assert!(
        scenario
            .plan
            .matrix
            .include
            .iter()
            .all(|entry| entry.artifact_id == *artifact_id)
    );
    assert_eq!(
        final_report.downloaded_artifact_ids.as_slice(),
        std::slice::from_ref(artifact_id)
    );
    Ok(())
}

#[test]
fn valid_selected_noop_reports_block_tasks() -> TestResult<()> {
    if process::child_mode(NOOP_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[("clippy", "crate_clippy"), ("test", "crate_test")])?;
    for (index, task_id) in scenario.plan.task_ids.iter().enumerate() {
        let digest = scenario.task_digest(task_id)?;
        assert_eq!(
            report_files::write_task(
                &scenario,
                NOOP_TEST,
                &format!("noop-{index}"),
                task_id,
                0,
                Some(digest),
            )?,
            1
        );
    }
    report_files::stage_all(&scenario)?;
    let jobs = [("crate_clippy", "success"), ("crate_test", "success")];
    let request = report_files::assemble(&scenario, NOOP_TEST, "noop", &jobs)?;
    let tasks = report_files::task_reports(&request)?;
    assert_eq!(tasks.len(), 2);
    assert!(
        tasks
            .iter()
            .all(|task| task.status == TaskStatus::NotSelected)
    );
    assert!(
        tasks
            .iter()
            .all(|task| { task.not_selected_reason == Some(NotSelectedReason::NotInPlan) })
    );
    let final_report = report_files::merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Blocked);
    assert_eq!(final_report.counts.blocked, 2);
    assert_eq!(final_report.counts.failed, 0);
    assert_eq!(final_report.counts.not_run, 0);
    Ok(())
}

#[test]
fn shared_job_artifact_is_downloaded_once() -> TestResult<()> {
    if process::child_mode(SHARED_ARTIFACT_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[("clippy", "crate_clippy"), ("test", "crate_clippy")])?;
    let entries = &scenario.plan.matrix.include;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].artifact_id, entries[1].artifact_id);
    assert_ne!(entries[0].matrix_key, entries[1].matrix_key);
    for (index, task_id) in scenario.plan.task_ids.iter().enumerate() {
        assert_eq!(
            report_files::write_task(
                &scenario,
                SHARED_ARTIFACT_TEST,
                &format!("shared-{index}"),
                task_id,
                0,
                None,
            )?,
            1
        );
    }
    report_files::stage_all(&scenario)?;
    let jobs = [("crate_clippy", "success")];
    let request = report_files::assemble(&scenario, SHARED_ARTIFACT_TEST, "shared", &jobs)?;
    let final_report = report_files::merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Passed);
    assert_eq!(final_report.counts.executed, 2);
    assert_eq!(
        final_report.downloaded_artifact_ids,
        [entries[0].artifact_id.clone()]
    );
    Ok(())
}

#[test]
fn provider_hit_does_not_reclassify_executed_task() -> TestResult<()> {
    if process::child_mode(PROVIDER_HIT_TEST)? {
        return Ok(());
    }
    let scenario = fixture::Scenario::new(&[("clippy", "crate_clippy")])?;
    let task_id = scenario.plan.task_ids[0].clone();
    assert_eq!(
        report_files::write_task(
            &scenario,
            PROVIDER_HIT_TEST,
            "provider-writer",
            &task_id,
            0,
            None
        )?,
        1
    );
    report_files::stage_all(&scenario)?;
    report_files::stage_provider_hit(&scenario)?;
    let jobs = [("crate_clippy", "success")];
    let request = report_files::assemble(&scenario, PROVIDER_HIT_TEST, "provider-merge", &jobs)?;
    let tasks = report_files::task_reports(&request)?;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].status, TaskStatus::Executed);
    assert_eq!(
        tasks[0].cache.layer,
        velnor_actions_contract::CacheLayer::TofuProviders
    );
    assert_eq!(
        tasks[0].cache.result,
        velnor_actions_contract::CacheResult::Hit
    );
    let final_report = report_files::merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Passed);
    assert_eq!(final_report.counts.selected, 1);
    assert_eq!(final_report.counts.executed, 1);
    assert_eq!(final_report.counts.reused, 0);
    assert_eq!(final_report.counts.failed, 0);
    assert_eq!(final_report.counts.cancelled, 0);
    assert_eq!(final_report.counts.blocked, 0);
    assert_eq!(final_report.counts.not_run, 0);
    assert_eq!(
        final_report.downloaded_artifact_ids,
        [scenario.plan.matrix.include[0].artifact_id.clone()]
    );
    Ok(())
}

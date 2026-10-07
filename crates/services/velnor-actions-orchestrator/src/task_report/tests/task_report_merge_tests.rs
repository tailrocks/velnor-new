//! Report-merge tests: staged producer output through assembly into merge.
//!
//! Declared via `#[path]` from `task_report.rs` under `cfg(test)`.
//! Plan fixtures live in the sibling `task_report_tests` module.

use super::*;

use std::fs;

use tempfile::TempDir;
use velnor_actions_contract_workflow::{FinalStatus, Plan};

use velnor_actions_orchestrator_internal::merge_entry::merge_internal;
use velnor_actions_orchestrator_merge_request::assemble_with_needs;
use velnor_actions_orchestrator_noop_report::noop_report::{
    NoOpRequest, write_noop_report_to, write_skip_reports,
};

/// Stage producer output as final-job `reports/<artifact-id>/` downloads.
///
/// Each job artifact carries the whole run directory, so the entry's
/// files stage under `<artifact-id>/<matrix-key>/`, mirroring the
/// crate-job upload path.
fn stage_downloads(plan: &Plan, temp: &TempDir) {
    let run = temp.path().join("velnor").join("local");
    for entry in &plan.matrix.include {
        let from = run.join(&entry.matrix_key);
        let home = run
            .join("reports")
            .join(&entry.artifact_id)
            .join(&entry.matrix_key);
        fs::create_dir_all(&home).expect("artifact dir");
        fs::rename(
            from.join("matrix-report.json"),
            home.join("matrix-report.json"),
        )
        .expect("stage matrix");
        let files = home.join("tasks");
        fs::create_dir_all(&files).expect("files dir");
        for file in fs::read_dir(from.join("tasks")).expect("tasks") {
            let file = file.expect("task entry").path();
            let name = file.file_name().expect("task name");
            fs::rename(&file, home.join("tasks").join(name)).expect("stage task");
        }
    }
}

/// Non-fork PR payload: the fixture plan is a PR plan, so assembly must
/// capture `pull_request` or trust coherence fails the merge closed.
const PR_PAYLOAD: &str = r#"{"pull_request":{"head":{"repo":{"fork":false}}}}"#;

#[test]
fn merge_flips_not_run_to_executed_end_to_end() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let run = temp.path().join("velnor").join("local");
    let needs = Some(r#"{"plan":"success"}"#);

    let bare = assemble_with_needs(
        "local",
        &run,
        needs,
        Some(r#"["plan"]"#),
        Some("pull_request"),
        Some(PR_PAYLOAD),
    )
    .expect("assemble bare");
    let verdict: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(&merge_internal(&bare).expect("merge bare")).expect("final json");
    eprintln!(
        "e2e before: status={:?} executed={} not_run={} downloaded={:?} miss={:?}",
        verdict.status,
        verdict.counts.executed,
        verdict.counts.not_run,
        verdict.downloaded_artifact_ids,
        verdict.miss_reasons
    );
    assert_eq!(verdict.status, FinalStatus::PlanningFailed);
    assert_eq!(verdict.counts.executed, 0);
    assert!(verdict.counts.not_run > 0);

    write_task_report_to("local", CLIPPY, 0, None, &[TEST.to_owned()], temp.path())
        .expect("clippy");
    write_task_report_to("local", TEST, 0, None, &[], temp.path()).expect("test");
    stage_downloads(&plan, &temp);

    let full = assemble_with_needs(
        "local",
        &run,
        needs,
        Some(r#"["plan"]"#),
        Some("pull_request"),
        Some(PR_PAYLOAD),
    )
    .expect("assemble full");
    assert!(
        !full.contains("missing_report"),
        "all artifacts staged: {full}"
    );
    let verdict: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(&merge_internal(&full).expect("merge full")).expect("final json");
    eprintln!(
        "e2e after: status={:?} executed={} not_run={} downloaded={:?}",
        verdict.status,
        verdict.counts.executed,
        verdict.counts.not_run,
        verdict.downloaded_artifact_ids
    );
    assert_eq!(verdict.status, FinalStatus::Passed);
    assert_eq!(verdict.counts.executed, 2);
    assert_eq!(verdict.counts.not_run, 0);
    assert_eq!(verdict.downloaded_artifact_ids.len(), 2);
}

#[test]
fn merge_fails_failing_obligation_and_blocks_downstream() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let run = temp.path().join("velnor").join("local");
    let needs = Some(r#"{"plan":"success"}"#);

    let reported =
        write_task_report_to("local", CLIPPY, 1, None, &[], temp.path()).expect("clippy fails");
    assert_eq!(reported, 1);
    let skipped = write_skip_reports(&plan, CLIPPY, &[TEST.to_owned()], temp.path())
        .expect("downstream skips");
    assert_eq!(skipped, 1);
    stage_downloads(&plan, &temp);

    let full = assemble_with_needs(
        "local",
        &run,
        needs,
        Some(r#"["plan"]"#),
        Some("pull_request"),
        Some(PR_PAYLOAD),
    )
    .expect("assemble full");
    assert!(
        !full.contains("missing_report"),
        "all artifacts staged: {full}"
    );
    let verdict: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(&merge_internal(&full).expect("merge full")).expect("final json");
    eprintln!(
        "e2e failure: status={:?} failed={} blocked={} not_run={}",
        verdict.status, verdict.counts.failed, verdict.counts.blocked, verdict.counts.not_run
    );
    assert_eq!(verdict.status, FinalStatus::Failed);
    assert_eq!(verdict.counts.failed, 1);
    assert_eq!(verdict.counts.blocked, 1);
    assert_eq!(verdict.counts.not_run, 0);
    assert_eq!(verdict.downloaded_artifact_ids.len(), 2);
}

#[test]
fn merge_blocks_all_skipped_noop_end_to_end() {
    let plan = fixture_plan();
    let temp = staged_run(&plan, "local");
    let run = temp.path().join("velnor").join("local");
    for (task, obligation) in [(CLIPPY, &plan.obligations[0]), (TEST, &plan.obligations[1])] {
        let request = NoOpRequest {
            reason: velnor_actions_contract_workflow::NotSelectedReason::NotInPlan,
            task_digest: obligation.task_digest.clone(),
        };
        write_noop_report_to(
            "local",
            task,
            0,
            &request,
            temp.path(),
            velnor_actions_orchestrator_retrieve::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES,
        )
        .expect("noop report");
    }
    stage_downloads(&plan, &temp);

    let full = assemble_with_needs(
        "local",
        &run,
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("pull_request"),
        Some(PR_PAYLOAD),
    )
    .expect("assemble full");
    assert!(
        !full.contains("missing_report"),
        "all artifacts staged: {full}"
    );
    let verdict: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(&merge_internal(&full).expect("merge full")).expect("final json");
    eprintln!(
        "e2e noop: status={:?} blocked={} not_run={}",
        verdict.status, verdict.counts.blocked, verdict.counts.not_run
    );
    assert_eq!(verdict.status, FinalStatus::Blocked);
    assert_eq!(verdict.counts.blocked, 2);
    assert_eq!(verdict.counts.failed, 0);
    assert_eq!(verdict.counts.not_run, 0);
}

#[test]
fn merge_lists_shared_job_artifact_once() {
    let mut plan = fixture_plan();
    let job_id = plan.matrix.include[0].job_id.clone();
    let shared = plan.matrix.include[0].artifact_id.clone();
    plan.matrix.include[1].job_id = job_id;
    plan.matrix.include[1].artifact_id = shared.clone();
    plan.validate().expect("shared-job plan validates");
    let temp = staged_run(&plan, "local");
    write_task_report_to("local", CLIPPY, 0, None, &[TEST.to_owned()], temp.path())
        .expect("clippy");
    write_task_report_to("local", TEST, 0, None, &[], temp.path()).expect("test");
    stage_downloads(&plan, &temp);
    let run = temp.path().join("velnor").join("local");
    let full = assemble_with_needs(
        "local",
        &run,
        Some(r#"{"plan":"success"}"#),
        Some(r#"["plan"]"#),
        Some("pull_request"),
        Some(PR_PAYLOAD),
    )
    .expect("assemble");
    assert!(
        !full.contains("missing_report"),
        "sibling entries share one artifact: {full}"
    );
    let verdict: velnor_actions_contract_workflow::FinalReport =
        serde_json::from_str(&merge_internal(&full).expect("merge")).expect("final json");
    assert_eq!(verdict.status, FinalStatus::Passed);
    assert_eq!(verdict.counts.executed, 2);
    assert_eq!(verdict.downloaded_artifact_ids, [shared]);
}

/// T23: a validate `TaskReport` that is `executed` alongside a
/// provider-cache hit aggregates to executed — the hit never mints
/// reuse and never disturbs the fold.
#[test]
fn provider_hit_validate_execution_aggregates_executed() {
    use std::collections::BTreeMap;
    use velnor_actions_contract::digest_b3;
    use velnor_actions_contract_workflow::{ExecuteTaskIds, MatrixStatus, Trust, WorkflowEvent};
    let plan = fixture_plan();
    let task_id = "stack/tofu/stacks/a/validate/default";
    let task_digest = digest_b3(b"tofu-validate-task");
    let entry = MatrixEntry::derive(
        "tofu",
        task_id,
        "true",
        &task_digest,
        serde_json::json!({}),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "validate".to_owned(),
                ExecuteTaskRef::Single(task_id.to_owned()),
            )]),
        },
        &digest_b3(b"tofu-validate-inputs"),
        "local",
        "tofu-stacks-a",
    )
    .expect("tofu entry derives");
    let task = TaskReport {
        schema: 1,
        task_report_id: task_report_id_for_task("local", &entry.matrix_key, &task_digest)
            .expect("report id"),
        run_key: "local".to_owned(),
        event: WorkflowEvent::PullRequest,
        trust: Trust::Pr,
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        task_id: task_id.to_owned(),
        task_digest,
        status: TaskStatus::Executed,
        not_selected_reason: None,
        cache: CacheOutcome {
            layer: CacheLayer::TofuProviders,
            key: "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-stacks-a-0123456789ab"
                .to_owned(),
            result: CacheResult::Hit,
            miss_reason: None,
        },
        exit_code: 0,
        duration_ms: None,
        outputs: vec![],
        lane: None,
        queue: None,
        partition: None,
        reason: None,
        timing: None,
    };
    task.validate().expect("executed+provider-hit validates");
    let aggregate = single_task_aggregate(&plan, &entry, &task).expect("aggregates");
    assert_eq!(aggregate.executed, 1);
    assert_eq!(aggregate.reused, 0);
    assert_eq!(aggregate.status, MatrixStatus::Passed);
}

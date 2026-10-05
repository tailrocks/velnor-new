use super::insert_gate_jobs;
use crate::cover::Signals;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    JobConclusion, MISE_PHASE0_LINUX_JOB_ID, MISE_PHASE0_MACOS_ARM64_JOB_ID,
    MISE_PHASE0_MACOS_INTEL_JOB_ID, RequiredJobResult, WorkflowPolicy,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::{FINAL_CONDITION, FINAL_JOB_ID};

#[test]
fn repository_phase0_jobs_are_required_fanin_dependencies() {
    let mut jobs = BTreeMap::new();
    insert_gate_jobs(
        &mut jobs,
        "ubuntu-26.04",
        "main",
        &[],
        None,
        &ToolCatalog::pinned(),
        WorkflowPolicy::VelnorRepositoryV1,
    )
    .expect("repository gate jobs");
    let required = &jobs[FINAL_JOB_ID];
    assert_eq!(required.condition.as_deref(), Some(FINAL_CONDITION));
    for id in [
        MISE_PHASE0_LINUX_JOB_ID,
        MISE_PHASE0_MACOS_ARM64_JOB_ID,
        MISE_PHASE0_MACOS_INTEL_JOB_ID,
    ] {
        assert!(jobs.contains_key(id), "missing required Phase0 job {id}");
        assert!(required.needs.iter().any(|need| need == id));
    }
}

#[test]
fn consumer_workflows_do_not_get_repository_phase0_jobs() {
    let mut jobs = BTreeMap::new();
    insert_gate_jobs(
        &mut jobs,
        "ubuntu-26.04",
        "main",
        &[],
        None,
        &ToolCatalog::pinned(),
        WorkflowPolicy::ConsumerV1,
    )
    .expect("consumer gate jobs");
    assert!(!jobs.contains_key(MISE_PHASE0_LINUX_JOB_ID));
    assert!(!jobs.contains_key(MISE_PHASE0_MACOS_ARM64_JOB_ID));
    assert!(!jobs.contains_key(MISE_PHASE0_MACOS_INTEL_JOB_ID));
}

#[test]
fn required_aggregator_marks_every_phase0_failure_and_cancellation_red() {
    let ids = [
        MISE_PHASE0_LINUX_JOB_ID,
        MISE_PHASE0_MACOS_ARM64_JOB_ID,
        MISE_PHASE0_MACOS_INTEL_JOB_ID,
    ];
    for id in ids {
        for (conclusion, expect_failed, expect_cancelled) in [
            (JobConclusion::Failure, true, false),
            (JobConclusion::Cancelled, false, true),
        ] {
            let mut signals = Signals::default();
            crate::required_evidence::fold_jobs(
                &[RequiredJobResult {
                    job_id: id.to_owned(),
                    conclusion,
                }],
                &mut signals,
            );
            assert_eq!(
                signals.failed, expect_failed,
                "failure signal for {id} {conclusion:?}"
            );
            assert_eq!(
                signals.cancelled, expect_cancelled,
                "cancellation signal for {id} {conclusion:?}"
            );
        }
    }
}

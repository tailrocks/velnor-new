//! Required-evidence fold verdicts for skipped, failed, and missing jobs.

use super::*;
use std::collections::BTreeSet;
use velnor_actions_contract_config::{
    ArtifactBuildOutput, ArtifactBuildTask, RunnerSelection, VerificationRunner,
};
use velnor_actions_contract_workflow::{
    ArtifactBuildIdentity, ArtifactBuildObservation, ArtifactBuildProvider,
    ArtifactBuildRunContext, ArtifactBuildTaskPlan, DownloadedArtifactOutput, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanRunner, Trust, WorkflowEvent, expected_artifact_builds,
    export_artifact_result,
};
use velnor_actions_orchestrator_merge_ports::MergeRequest;

fn artifact_plan() -> Plan {
    let run_key = "r7-a1";
    let task = ArtifactBuildTask {
        id: "bundle".to_owned(),
        mise_task: "build-bundle".to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 15,
        outputs: vec![ArtifactBuildOutput {
            id: "archive".to_owned(),
            path: "dist/archive.tar".to_owned(),
            max_bytes: 1024,
        }],
    };
    Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.to_owned(),
        plan_id: "plan-r7-a1".to_owned(),
        base: None,
        head: "a".repeat(40),
        event: WorkflowEvent::Push,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Trusted,
        baseline: PlanBaseline::unavailable(Some("no_baseline")).expect("baseline"),
        generator: PlanGenerator {
            version: "0.1.1".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: Vec::new(),
        obligations: Vec::new(),
        matrix: PlanMatrix {
            include: Vec::new(),
        },
        task_ids: Vec::new(),
        artifact_tasks: vec![ArtifactBuildTaskPlan {
            task,
            providers: vec![
                ArtifactBuildProvider::GithubHosted,
                ArtifactBuildProvider::VelnorScaleSet,
            ],
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

fn artifact_context() -> ArtifactBuildRunContext {
    ArtifactBuildRunContext {
        repository_id: "123".to_owned(),
        repository: "owner/repo".to_owned(),
        run_id: "7".to_owned(),
        run_attempt: 1,
    }
}

fn artifact_observation(
    task: &ArtifactBuildTask,
    identity: ArtifactBuildIdentity,
) -> ArtifactBuildObservation {
    let bytes = b"built output".to_vec();
    let result = export_artifact_result(
        identity.clone(),
        task,
        &[("archive".to_owned(), bytes.clone())],
    )
    .expect("typed result");
    let name = result.artifact_name.clone();
    let velnor = identity.provider == ArtifactBuildProvider::VelnorScaleSet;
    ArtifactBuildObservation {
        identity: identity.clone(),
        api_run_id: identity.run_id.clone(),
        api_head_sha: identity.source_sha.clone(),
        api_job_id: Some(if velnor { 102 } else { 101 }),
        api_job_name: identity.provider.workflow_job_name(&identity.task_id),
        api_job_status: "completed".to_owned(),
        api_runner_id: velnor.then_some(301),
        api_runner_name: velnor.then(|| "velnor-runner-301".to_owned()),
        api_runner_group_id: velnor.then_some(12),
        api_runner_group_name: velnor.then(|| "ChainArgos Scale Set".to_owned()),
        api_runner_labels: if velnor {
            vec!["self-hosted".to_owned(), "velnor".to_owned()]
        } else {
            vec!["ubuntu-26.04".to_owned()]
        },
        api_artifact_id: Some(if velnor { 202 } else { 201 }),
        api_artifact_name: Some(name),
        api_artifact_size_bytes: Some(1024),
        api_artifact_expired: Some(false),
        api_artifact_run_id: Some(identity.run_id.clone()),
        api_artifact_repository_id: Some(identity.repository_id.clone()),
        api_artifact_head_sha: Some(identity.source_sha.clone()),
        conclusion: JobConclusion::Success,
        result: Some(result),
        downloaded_outputs: vec![DownloadedArtifactOutput {
            output_id: "archive".to_owned(),
            path: "dist/archive.tar".to_owned(),
            size_bytes: u64::try_from(bytes.len()).expect("size"),
            digest: velnor_actions_contract::canonical::digest_b3(&bytes),
        }],
    }
}

fn artifact_request(
    context: Option<ArtifactBuildRunContext>,
    observations: Vec<ArtifactBuildObservation>,
) -> MergeRequest {
    MergeRequest {
        schema: 1,
        run_key: "r7-a1".to_owned(),
        actual_event: None,
        candidate_attestation: None,
        artifact_build_context: context,
        artifact_build_observations: observations,
        task_report_outputs: None,
        plan: None,
        matrix: None,
        matrix_reports: Vec::new(),
        task_reports: Vec::new(),
        check_proofs: Vec::new(),
        required_job_ids: vec!["plan".to_owned()],
        required_jobs: vec![RequiredJobResult {
            job_id: "plan".to_owned(),
            conclusion: JobConclusion::Success,
        }],
        assembly_errors: Vec::new(),
        baseline_manifest: None,
        shard_proofs: Vec::new(),
        limits: None,
        reference_task_ids: None,
    }
}

#[test]
fn skipped_check_marks_not_run() {
    let mut signals = Signals::default();
    fold_jobs(
        &[RequiredJobResult {
            job_id: "check-native".to_owned(),
            conclusion: JobConclusion::Skipped,
        }],
        &mut signals,
    );
    assert!(signals.not_run);
}

#[test]
fn required_rejects_failed_or_missing_named_check() {
    for conclusion in [JobConclusion::Missing, JobConclusion::Failure] {
        let mut signals = Signals::default();
        fold_jobs(
            &[RequiredJobResult {
                job_id: "check-ffi".to_owned(),
                conclusion,
            }],
            &mut signals,
        );
        assert!(signals.failed, "{conclusion:?}");
    }
}

#[test]
fn required_artifact_build_gate_accepts_exact_successful_provider_pair() {
    let plan = artifact_plan();
    plan.validate().expect("authoritative plan");
    let context = artifact_context();
    let providers = [
        ArtifactBuildProvider::GithubHosted,
        ArtifactBuildProvider::VelnorScaleSet,
    ];
    let expected = expected_artifact_builds(&plan, &context, &providers).expect("inventory");
    let task = &plan.artifact_tasks[0].task;
    let observations = expected
        .into_iter()
        .map(|item| artifact_observation(task, item.identity))
        .collect();
    let request = artifact_request(Some(context), observations);
    let mut signals = Signals::default();
    let mut reasons = BTreeSet::new();

    check_required_evidence(&plan, &request, &mut signals, &mut reasons);

    assert!(!signals.planning_failed);
    assert!(!signals.failed);
    assert!(!signals.cancelled);
    assert!(!signals.not_run);
    assert!(reasons.is_empty());
}

#[test]
fn required_artifact_build_gate_rejects_missing_and_skipped_lanes() {
    let plan = artifact_plan();
    let context = artifact_context();
    let hosted_provider = ArtifactBuildProvider::GithubHosted;
    let hosted_expected = expected_artifact_builds(
        &plan,
        &context,
        &[hosted_provider, ArtifactBuildProvider::VelnorScaleSet],
    )
    .expect("inventory")[0]
        .clone();
    let task = &plan.artifact_tasks[0].task;
    let hosted = artifact_observation(task, hosted_expected.identity);

    let missing = artifact_request(Some(context.clone()), vec![hosted.clone()]);
    let mut missing_signals = Signals::default();
    let mut missing_reasons = BTreeSet::new();
    check_required_evidence(&plan, &missing, &mut missing_signals, &mut missing_reasons);
    assert!(missing_signals.planning_failed);
    assert!(missing_reasons.contains("no_entry"));

    let velnor_expected = expected_artifact_builds(
        &plan,
        &context,
        &[
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
    )
    .expect("inventory")[1]
        .clone();
    let mut skipped = artifact_observation(task, velnor_expected.identity);
    skipped.api_job_status = "queued".to_owned();
    skipped.conclusion = JobConclusion::Missing;
    let request = artifact_request(Some(context), vec![hosted, skipped]);
    let mut signals = Signals::default();
    let mut reasons = BTreeSet::new();
    check_required_evidence(&plan, &request, &mut signals, &mut reasons);
    assert!(signals.not_run);
    assert!(!signals.planning_failed);
}

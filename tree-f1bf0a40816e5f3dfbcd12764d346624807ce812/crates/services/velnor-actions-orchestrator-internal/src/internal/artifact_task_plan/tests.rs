use super::*;
use velnor_actions_contract_config::{
    ArtifactBuildOutput, ExecutionOverride, ExecutionRole, VerificationRunner,
};
use velnor_actions_contract_workflow::{
    Concurrency, Job, JobTimeout, Permissions, Trigger, WorkflowIr,
};

fn output() -> ArtifactBuildOutput {
    ArtifactBuildOutput {
        id: "lint-report".to_owned(),
        path: "dist/lint-report.json".to_owned(),
        max_bytes: 4096,
    }
}

fn verification(id: &str, outputs: Vec<ArtifactBuildOutput>) -> VerificationTask {
    VerificationTask {
        id: id.to_owned(),
        kind: velnor_actions_contract_config::VerificationTaskKind::Verification,
        mise_task: format!("lint-{id}"),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 20,
        outputs,
    }
}

fn job(id: &str) -> Job {
    Job {
        display_name: format!("Verify {id}"),
        runs_on: VerificationRunner::LinuxX64.runs_on().to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        outputs: Vec::new(),
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: Vec::new(),
    }
}

fn workflow(ids: &[&str]) -> WorkflowIr {
    WorkflowIr {
        name: "test".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "test".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs: ids
            .iter()
            .map(|id| (format!("task-{id}"), job(id)))
            .collect(),
    }
}

fn hosted_default() -> ExecutionConfig {
    ExecutionConfig::hosted_default("ubuntu-26.04").expect("valid profiles")
}

fn override_for(profile: &str) -> ExecutionOverride {
    ExecutionOverride {
        profile: profile.to_owned(),
        role: ExecutionRole::Verification,
    }
}

#[test]
fn matrix_and_verification_tasks_keep_their_own_provider_scopes() {
    let matrix = ArtifactBuildTask {
        id: "image-build".to_owned(),
        mise_task: "build-image".to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 30,
        outputs: vec![output()],
    };
    let execution = hosted_default();
    let verification_tasks = [verification("frontend-check", vec![output()])];
    let workflow = workflow(&["frontend-check"]);
    let planned = planned_artifact_tasks(
        &[matrix],
        &verification_tasks,
        &[
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ],
        2,
        Some(&execution),
        None,
        &workflow,
    )
    .expect("planned artifact tasks");

    assert_eq!(planned.len(), 2);
    assert_eq!(planned[0].task.id, "frontend-check");
    assert_eq!(planned[0].producer, ArtifactBuildProducer::VerificationTask);
    assert_eq!(planned[0].task.mise_task, "lint-frontend-check");
    assert_eq!(planned[0].task.outputs, [output()]);
    assert_eq!(planned[0].providers, [ArtifactBuildProvider::GithubHosted]);
    assert_eq!(planned[1].task.id, "image-build");
    assert_eq!(planned[1].producer, ArtifactBuildProducer::MatrixBuild);
    assert_eq!(
        planned[1].providers,
        [
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ]
    );
}

#[test]
fn per_task_overrides_drive_static_output_providers_through_lane_placement() {
    let mut execution = hosted_default();
    execution.overrides.insert(
        "task-scale-check".to_owned(),
        override_for(&execution.scale_set_profile),
    );
    let tasks = [
        verification("hosted-check", vec![output()]),
        verification("scale-check", vec![output()]),
    ];
    let workflow = workflow(&["hosted-check", "scale-check"]);
    let planned = planned_artifact_tasks(&[], &tasks, &[], 2, Some(&execution), None, &workflow)
        .expect("per-task profile overrides are resolved");

    assert_eq!(planned.len(), 2);
    assert_eq!(planned[0].task.id, "hosted-check");
    assert_eq!(planned[0].providers, [ArtifactBuildProvider::GithubHosted]);
    assert_eq!(planned[1].task.id, "scale-check");
    assert_eq!(
        planned[1].providers,
        [ArtifactBuildProvider::VelnorScaleSet]
    );
}

#[test]
fn explicit_both_mode_records_both_static_output_producers() {
    let mut execution = hosted_default();
    execution.mode = Some(ExecutionMode::Both);
    let tasks = [verification("paired-check", vec![output()])];
    let workflow = workflow(&["paired-check"]);
    let planned = planned_artifact_tasks(&[], &tasks, &[], 2, Some(&execution), None, &workflow)
        .expect("both mode is resolved by lane placement");

    assert_eq!(
        planned[0].providers,
        [
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ]
    );
}

#[test]
fn schema_one_static_output_remains_hosted_without_schema_two_execution() {
    let tasks = [verification("legacy-check", vec![output()])];
    let workflow = workflow(&["legacy-check"]);
    let planned = planned_artifact_tasks(&[], &tasks, &[], 1, None, None, &workflow)
        .expect("schema one is hosted-only");

    assert_eq!(planned[0].providers, [ArtifactBuildProvider::GithubHosted]);
}

#[test]
fn empty_verification_output_inventory_adds_no_artifact_producer() {
    let tasks = [verification("frontend-check", Vec::new())];
    let workflow = workflow(&["frontend-check"]);
    let planned = planned_artifact_tasks(&[], &tasks, &[], 2, None, None, &workflow)
        .expect("empty outputs do not resolve lane providers");
    assert!(planned.is_empty());
}

use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract::{
    CacheMode, Concurrency, ExecutionMode, Job, JobTimeout, MiseTaskSource, Permissions,
    ServiceContainer, Step, StepKind, Trigger, VerificationRunner, VerificationTask, WorkflowIr,
};

use super::{
    WorkflowRoutingPolicy, selector, validate_generated_workflow_calls,
    validate_generated_workflow_calls_with_ir, workflow,
};
use crate::verification_jobs::{
    VerificationTaskPolicy, WorkflowTaskPolicy, build_verification_task_job,
};
use crate::{MiseSetup, RenderedFile};

#[test]
fn typed_native_check_needs_the_matching_ir_and_rendered_fields() {
    let selector = selector();
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::Hosted, selector, None);
    let workflow_ir = native_check_ir();
    let file = workflow(
        "ci",
        &format!(
            "on: push\njobs:\n  check-native:\n    runs-on: native-scale-set\n    if: \"{EPHEMERAL_CHECK_ADMISSION_CONDITION}\"\n    steps: []\n"
        ),
    );
    assert!(validate_generated_workflow_calls(std::slice::from_ref(&file), &policy).is_err());
    assert_eq!(
        validate_generated_workflow_calls_with_ir(
            std::slice::from_ref(&file),
            &policy,
            &workflow_ir,
        ),
        Ok(())
    );

    let altered = workflow(
        "ci",
        &format!(
            "on: push\njobs:\n  check-native:\n    runs-on: another-runner\n    if: \"{EPHEMERAL_CHECK_ADMISSION_CONDITION}\"\n    steps: []\n"
        ),
    );
    assert!(validate_generated_workflow_calls_with_ir(&[altered], &policy, &workflow_ir).is_err());
}

fn native_check_ir() -> WorkflowIr {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "check-native".to_owned(),
        Job {
            services: Vec::new(),
            display_name: "Check / native".to_owned(),
            runs_on: "native-scale-set".to_owned(),
            check_runner: Some(CheckRunner {
                label: "native-scale-set".to_owned(),
                platform: CheckPlatform::MacosArm64,
                executor: CheckExecutor::EphemeralSelfHosted,
                container: None,
            }),
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()),
            permissions: None,
            cache_mode: None,
            environment: None,
            steps: vec![Step {
                name: "Run native check".to_owned(),
                id: None,
                role: None,
                condition: None,
                parallel: false,
                kind: StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: BTreeMap::new(),
                },
            }],
        },
    );
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs,
    }
}

#[test]
fn declared_macos_task_routes_hosted_only_with_exact_source_ir() {
    let (workflow_ir, tasks) = native_task_fixture();
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None)
        .with_verified_workflow_tasks(&workflow_ir, &tasks, CHECKOUT)
        .expect("the source task is validated before routing is authorized");
    let file = native_task_file("runs-on: macos-15\n    steps: []\n");

    assert!(validate_generated_workflow_calls(std::slice::from_ref(&file), &policy).is_err());
    assert_eq!(
        validate_generated_workflow_calls_with_ir(
            std::slice::from_ref(&file),
            &policy,
            &workflow_ir,
        ),
        Ok(())
    );

    let mut changed_ir = workflow_ir.clone();
    changed_ir
        .jobs
        .get_mut("task-native-format")
        .expect("native task job")
        .display_name = "Forged native task".to_owned();
    assert!(
        validate_generated_workflow_calls_with_ir(&[file], &policy, &changed_ir).is_err(),
        "the final IR must still match the source job that earned hosted admission"
    );
}

#[test]
fn declared_macos_task_policy_cannot_authorize_a_noncanonical_source_job() {
    let (mut workflow_ir, tasks) = native_task_fixture();
    workflow_ir
        .jobs
        .get_mut("task-native-format")
        .expect("native task job")
        .runs_on = "macos-26".to_owned();
    let rejects_noncanonical = |workflow: &WorkflowIr| {
        WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None)
            .with_verified_workflow_tasks(workflow, &tasks, CHECKOUT)
            .is_err()
    };
    assert!(rejects_noncanonical(&workflow_ir));
    let rejects_full_snapshot_mismatch = |workflow: &WorkflowIr| {
        matches!(
            WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None)
                .with_verified_workflow_tasks(workflow, &tasks, CHECKOUT),
            Err(crate::RenderError::InvalidWorkflow(problem))
                if problem == "native_hosted_task_source_job_invalid:task-native-format"
        )
    };

    let mut altered = native_task_fixture().0;
    altered
        .jobs
        .get_mut("task-native-format")
        .expect("native task job")
        .services
        .push(ServiceContainer::postgres(18).expect("catalog service"));
    assert!(
        rejects_full_snapshot_mismatch(&altered),
        "service changes are not canonical"
    );

    let mut altered = native_task_fixture().0;
    altered
        .jobs
        .get_mut("task-native-format")
        .expect("native task job")
        .cache_mode = Some(CacheMode::Read);
    assert!(
        rejects_full_snapshot_mismatch(&altered),
        "cache policy is not canonical"
    );

    let mut altered = native_task_fixture().0;
    altered
        .jobs
        .get_mut("task-native-format")
        .expect("native task job")
        .check_runner = Some(CheckRunner {
        label: VerificationRunner::MacosArm64.runs_on().to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::Hosted,
        container: None,
    });
    // Workflow validation rejects this job-ID/field combination before admission.
    let mut proofs = BTreeMap::new();
    assert!(
        matches!(
            super::super::super::native_tasks::bind_native_hosted_tasks(
                &mut proofs,
                &altered,
                &tasks,
                CHECKOUT,
            ),
            Err(crate::RenderError::InvalidWorkflow(problem))
                if problem == "native_hosted_task_source_job_invalid:task-native-format"
        ),
        "a check-runner role is not canonical"
    );
}

#[test]
fn declared_macos_task_must_remain_in_the_exact_ci_job_inventory() {
    let (workflow_ir, tasks) = native_task_fixture();
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None)
        .with_verified_workflow_tasks(&workflow_ir, &tasks, CHECKOUT)
        .expect("valid native task fixture");
    let missing = workflow(
        "ci",
        "on: push\njobs:\n  unrelated:\n    runs-on: ubuntu-26.04\n    steps: []\n",
    );

    assert!(
        validate_generated_workflow_calls_with_ir(&[missing], &policy, &workflow_ir).is_err(),
        "the exact task job must be present in the final CI workflow"
    );
}

#[test]
fn declared_macos_task_cannot_be_moved_to_another_workflow_file() {
    let (workflow_ir, tasks) = native_task_fixture();
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None)
        .with_verified_workflow_tasks(&workflow_ir, &tasks, CHECKOUT)
        .expect("valid native task fixture");
    let misplaced = workflow(
        "other",
        "on: push\njobs:\n  task-native-format:\n    runs-on: macos-15\n    steps: []\n",
    );

    assert!(
        validate_generated_workflow_calls_with_ir(&[misplaced], &policy, &workflow_ir).is_err(),
        "only the exact CI workflow can contain the declared native task"
    );
}

#[test]
fn declared_macos_task_rejects_forged_yaml_runner_fields_and_reusable_calls() {
    let (workflow_ir, tasks) = native_task_fixture();
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None)
        .with_verified_workflow_tasks(&workflow_ir, &tasks, CHECKOUT)
        .expect("valid native task fixture");

    for job in [
        "runs-on: macos-26\n    steps: []\n",
        "runs-on: macos-15\n    if: github.ref == 'refs/heads/main'\n    steps: []\n",
        "runs-on: macos-15\n    container: ubuntu:24.04\n    steps: []\n",
        "runs-on: macos-15\n    strategy: {matrix: {runner: [macos-15]}}\n    steps: []\n",
        "uses: ./.github/workflows/reusable.yml\n",
    ] {
        let file = native_task_file(job);
        assert!(
            validate_generated_workflow_calls_with_ir(
                std::slice::from_ref(&file),
                &policy,
                &workflow_ir,
            )
            .is_err(),
            "forged native task fields must be rejected: {job}"
        );
    }
}

const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

fn native_task_fixture() -> (WorkflowIr, Vec<WorkflowTaskPolicy>) {
    let verification = VerificationTaskPolicy {
        task: VerificationTask {
            id: "native-format".to_owned(),
            mise_task: "format-check".to_owned(),
            source: MiseTaskSource::default(),
            runner: VerificationRunner::MacosArm64,
            timeout_minutes: 5,
        },
        runner_label: VerificationRunner::MacosArm64.runs_on().to_owned(),
        scale_set_token: Some(selector().token()),
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            version: "2026.10.18".to_owned(),
            sha256: "a".repeat(64),
        },
        selected_tools: Vec::new(),
        mise_config_sha256: Some("b".repeat(64)),
        mise_lock_sha256: None,
        rust_toolchain_sha256: None,
    };
    let job = build_verification_task_job(&verification, CHECKOUT).expect("canonical task job");
    let tasks = vec![WorkflowTaskPolicy::Verification(verification)];
    let workflow = WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        cache_mode: None,
        concurrency: Concurrency {
            group: "ci".to_owned(),
            cancel_in_progress: "true".to_owned(),
        },
        jobs: BTreeMap::from([("task-native-format".to_owned(), job)]),
    };
    (workflow, tasks)
}

fn native_task_file(job: &str) -> RenderedFile {
    RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: format!("on: push\njobs:\n  task-native-format:\n    {job}"),
    }
}

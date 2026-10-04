use super::{NativePagesActions, NativePagesDeploy, NativePagesTriggerPolicy};
use crate::workflow::{
    ir::{Concurrency, Job, Trigger, WorkflowIr},
    outputs::{ActionOutput, JobOutput, StepOutputRef},
    permissions::{PermissionLevel, Permissions},
    source_helper::{HelperInvocation, SourceBoundHelper, SourceBoundOperation},
    step::{Step, StepId, StepKind},
    timeout::JobTimeout,
};
use std::collections::BTreeMap;

fn invocation() -> HelperInvocation {
    let operation = SourceBoundOperation::NativePagesAdmission;
    HelperInvocation::compiled(
        SourceBoundHelper::compiled(operation, operation.path(), &"a".repeat(64)).expect("source"),
        vec!["verify".to_owned()],
        Vec::new(),
    )
    .expect("invocation")
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        cache_mode: None,
        display_name: "Proof".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps,
    }
}

fn action(uses: &str, id: Option<&str>, with: BTreeMap<String, String>) -> Step {
    Step {
        id: id.map(|id| StepId::new(id).expect("id")),
        name: "Action".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
            env: BTreeMap::new(),
        },
    }
}

fn role() -> NativePagesDeploy {
    let pin = |owner: &str| format!("{owner}@{}", "a".repeat(40));
    NativePagesDeploy {
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        trigger_policy: NativePagesTriggerPolicy::ProtectedPushDispatch,
        preparation: Vec::new(),
        full_ci_job: "admit".to_owned(),
        full_ci_admission: HelperInvocation::compiled(
            SourceBoundHelper::compiled(
                SourceBoundOperation::ReleaseAdmissionDefaultBranch,
                SourceBoundOperation::ReleaseAdmissionDefaultBranch.path(),
                &"a".repeat(64),
            )
            .expect("source"),
            Vec::new(),
            Vec::new(),
        )
        .expect("proof"),
        artifact_job: "stage".to_owned(),
        artifact_id_output: "artifact_id".to_owned(),
        artifact_digest_output: "artifact_digest".to_owned(),
        admission_step: StepId::new("guard").expect("id"),
        admission: invocation(),
        admission_extra_environment: BTreeMap::new(),
        deploy_step: StepId::new("deploy").expect("id"),
        actions: NativePagesActions {
            checkout: pin("actions/checkout"),
            configure: pin("actions/configure-pages"),
            upload: pin("actions/upload-pages-artifact"),
            deploy: pin("actions/deploy-pages"),
        },
    }
}

fn producers(role: &NativePagesDeploy) -> (Job, Job) {
    let pin = |owner: &str| format!("{owner}@{}", "a".repeat(40));
    let admit = job(vec![Step {
        id: None,
        name: "Full CI proof".to_owned(),
        condition: None,
        kind: StepKind::SourceBoundHelper {
            invocation: role.full_ci_admission.clone(),
            env: BTreeMap::new(),
        },
    }]);
    let mut stage = job(vec![action(
        &pin("actions/upload-artifact"),
        Some("upload"),
        BTreeMap::new(),
    )]);
    stage.outputs = [
        (&role.artifact_id_output, ActionOutput::ArtifactId),
        (&role.artifact_digest_output, ActionOutput::ArtifactDigest),
    ]
    .into_iter()
    .map(|(name, output)| JobOutput {
        name: name.clone(),
        value: StepOutputRef {
            step_id: StepId::new("upload").expect("id"),
            output,
        },
    })
    .collect();
    (admit, stage)
}

fn deploy(role: NativePagesDeploy) -> Job {
    let mut deploy = job(vec![
        action(
            &role.actions.checkout,
            None,
            BTreeMap::from([
                ("persist-credentials".to_owned(), "false".to_owned()),
                ("ref".to_owned(), "${{ github.sha }}".to_owned()),
            ]),
        ),
        Step {
            id: Some(role.admission_step.clone()),
            name: "Guard".to_owned(),
            condition: None,
            kind: StepKind::SourceBoundHelper {
                invocation: role.admission.clone(),
                env: role.admission_environment(),
            },
        },
        action(&role.actions.configure, None, BTreeMap::new()),
        action(
            &role.actions.upload,
            None,
            BTreeMap::from([("path".to_owned(), "public".to_owned())]),
        ),
        action(&role.actions.deploy, Some("deploy"), BTreeMap::new()),
    ]);
    deploy.needs = vec!["admit".to_owned(), "stage".to_owned()];
    deploy.condition = Some(role.condition());
    deploy.environment = Some("github-pages".to_owned());
    deploy.permissions = Some(Permissions {
        contents: PermissionLevel::Read,
        actions: PermissionLevel::Read,
        id_token: PermissionLevel::Write,
        pages: PermissionLevel::Write,
        attestations: PermissionLevel::None,
        issues: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
    });
    deploy.native_pages_deploy = Some(role);
    deploy
}

fn fixture() -> WorkflowIr {
    let role = role();
    let (admit, stage) = producers(&role);
    let deploy = deploy(role);
    WorkflowIr {
        cache_mode: crate::workflow::CacheMode::Read,
        name: "Pages".to_owned(),
        run_name: None,
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: vec!["main".to_owned()],
            push_tags: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "pages".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs: BTreeMap::from([
            ("admit".to_owned(), admit),
            ("stage".to_owned(), stage),
            ("deploy".to_owned(), deploy),
        ]),
    }
}

#[test]
fn native_pages_rejects_permission_environment_and_conditional_bypasses() {
    assert!(fixture().validate().is_ok());
    for index in 0..5 {
        let mut workflow = fixture();
        workflow.jobs.get_mut("deploy").expect("job").steps[index].condition =
            Some("false".to_owned());
        assert!(workflow.validate().is_err(), "{index}");
    }
    let mut workflow = fixture();
    let deploy = workflow.jobs.get_mut("deploy").expect("job");
    deploy.permissions.as_mut().expect("permissions").contents = PermissionLevel::Write;
    assert!(workflow.validate().is_err());
    let mut workflow = fixture();
    workflow.jobs.get_mut("deploy").expect("job").environment = Some("arbitrary".to_owned());
    assert!(workflow.validate().is_err());
    let mut workflow = fixture();
    workflow
        .jobs
        .get_mut("deploy")
        .expect("job")
        .native_pages_deploy = None;
    assert!(workflow.validate().is_err());
}

#[test]
fn native_pages_requires_exact_source_artifact_full_ci_and_sequence() {
    let mut workflow = fixture();
    workflow
        .jobs
        .get_mut("deploy")
        .expect("job")
        .needs
        .retain(|need| need != "admit");
    assert!(workflow.validate().is_err());
    let mut workflow = fixture();
    workflow.jobs.get_mut("admit").expect("job").steps[0].kind = StepKind::Internal {
        operation: "noop".to_owned(),
    };
    assert!(workflow.validate().is_err());
    let mut workflow = fixture();
    workflow
        .jobs
        .get_mut("stage")
        .expect("job")
        .outputs
        .remove(1);
    assert!(workflow.validate().is_err());
    let mut workflow = fixture();
    if let StepKind::SourceBoundHelper { env, .. } =
        &mut workflow.jobs.get_mut("deploy").expect("job").steps[1].kind
    {
        env.insert("APPROVED_SOURCE_SHA".to_owned(), "other".to_owned());
    }
    assert!(workflow.validate().is_err());
    let mut workflow = fixture();
    workflow
        .jobs
        .get_mut("deploy")
        .expect("job")
        .steps
        .swap(1, 4);
    assert!(workflow.validate().is_err());
}

#[test]
fn native_pages_dispatch_cannot_bypass_publish_mode_or_protected_branch() {
    let mut workflow = fixture();
    workflow.triggers.schedule = Some(crate::workflow::jobs::ScheduleTrigger {
        cron: vec!["0 6 * * 1".to_owned()],
    });
    let deploy = workflow.jobs.get_mut("deploy").expect("job");
    let role = deploy.native_pages_deploy.as_mut().expect("role");
    role.trigger_policy = NativePagesTriggerPolicy::ScheduledExplicitPublish;
    deploy.condition = Some(role.condition());
    assert!(
        deploy
            .condition
            .as_ref()
            .expect("condition")
            .contains("inputs.mode == 'publish'")
    );
    assert!(workflow.validate().is_ok());
    for condition in [
        "github.event_name == 'workflow_dispatch'",
        "success()",
        "true",
    ] {
        workflow.jobs.get_mut("deploy").expect("job").condition = Some(condition.to_owned());
        assert!(workflow.validate().is_err());
    }
}

#[test]
fn native_pages_preparation_order_and_exact_invocation_are_mandatory() {
    use super::NativePagesPreparation;
    let mut workflow = fixture();
    let deploy = workflow.jobs.get_mut("deploy").expect("job");
    let role = deploy.native_pages_deploy.as_mut().expect("role");
    for (id, operation) in [
        ("bootstrap", SourceBoundOperation::MiseBootstrap),
        ("tools", SourceBoundOperation::MiseToolPrepare),
    ] {
        let invocation = HelperInvocation::compiled(
            SourceBoundHelper::compiled(operation, operation.path(), &"a".repeat(64))
                .expect("source"),
            Vec::new(),
            Vec::new(),
        )
        .expect("invocation");
        role.preparation.push(NativePagesPreparation {
            step_id: StepId::new(id).expect("id"),
            invocation,
            environment: BTreeMap::new(),
        });
    }
    let preparations = role
        .preparation
        .iter()
        .map(|preparation| Step {
            id: Some(preparation.step_id.clone()),
            name: "Prepare".to_owned(),
            condition: None,
            kind: StepKind::SourceBoundHelper {
                invocation: preparation.invocation.clone(),
                env: preparation.environment.clone(),
            },
        })
        .collect::<Vec<_>>();
    drop(deploy.steps.splice(1..1, preparations));
    assert!(workflow.validate().is_ok());
    workflow
        .jobs
        .get_mut("deploy")
        .expect("job")
        .steps
        .swap(1, 2);
    assert!(workflow.validate().is_err());
    workflow
        .jobs
        .get_mut("deploy")
        .expect("job")
        .steps
        .swap(1, 2);
    workflow
        .jobs
        .get_mut("deploy")
        .expect("job")
        .steps
        .remove(1);
    assert!(workflow.validate().is_err());
}

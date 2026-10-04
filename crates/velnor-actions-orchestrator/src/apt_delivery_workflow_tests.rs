//! Typed APT graph privilege, binding, and trigger tests.

use std::collections::BTreeMap;

use super::super::tests::{config, context};
use velnor_actions_contract::{
    ActionOutput, Job, SourceBoundOperation, Step, StepId, StepKind,
    workflow::{
        PermissionLevel,
        dispatch::{DispatchInput, DispatchInputType},
    },
};

fn graph() -> super::AptWorkflow {
    super::workflow(&config(), &context()).expect("APT graph")
}

fn env(step: &Step) -> Option<&BTreeMap<String, String>> {
    match &step.kind {
        StepKind::Action { env, .. }
        | StepKind::Shell { env, .. }
        | StepKind::SourceBoundHelper { env, .. } => Some(env),
        StepKind::Internal { .. } => None,
    }
}

fn named_step<'a>(job: &'a Job, name: &str) -> &'a Step {
    job.steps
        .iter()
        .find(|step| step.name == name)
        .unwrap_or_else(|| panic!("missing step {name}"))
}

fn input<'a>(inputs: &'a [DispatchInput], name: &str) -> &'a DispatchInput {
    inputs
        .iter()
        .find(|input| input.name == name)
        .unwrap_or_else(|| panic!("missing dispatch input {name}"))
}

#[test]
fn typed_graph_keeps_signing_and_pages_authority_separate() {
    let graph = graph();
    graph.ir.validate().expect("valid APT graph");

    let verify = &graph.ir.jobs["verify"];
    let admit = &graph.ir.jobs["admit"];
    let stage = &graph.ir.jobs["stage"];
    let deploy = &graph.ir.jobs["deploy"];

    assert_eq!(stage.environment.as_deref(), Some("package-feed"));
    assert_eq!(deploy.environment.as_deref(), Some("github-pages"));
    assert_eq!(graph.pages_approvals.len(), 1);

    let signing = env(named_step(stage, "Reverify and stage the signed feed")).expect("sign env");
    assert_eq!(
        signing.get("APT_GPG_PRIVATE_KEY").map(String::as_str),
        Some("${{ secrets.APT_GPG_PRIVATE_KEY }}")
    );
    assert_eq!(
        signing.get("APT_GPG_PASSPHRASE").map(String::as_str),
        Some("${{ secrets.APT_GPG_PASSPHRASE }}")
    );

    for job in [verify, admit, deploy, &graph.ir.jobs["feed-result"]] {
        assert!(!job.steps.iter().any(|step| {
            env(step).is_some_and(|values| values.keys().any(|key| key.starts_with("APT_GPG_")))
        }));
    }

    for permissions in [
        graph.ir.jobs["stage"].permissions.as_ref(),
        graph.ir.jobs["admit"].permissions.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        assert_eq!(permissions.contents, PermissionLevel::Read);
        assert_eq!(permissions.actions, PermissionLevel::Read);
        assert_eq!(permissions.pages, PermissionLevel::None);
        assert_eq!(permissions.id_token, PermissionLevel::None);
    }
    let pages = deploy.permissions.as_ref().expect("Pages permissions");
    assert_eq!(pages.contents, PermissionLevel::Read);
    assert_eq!(pages.actions, PermissionLevel::Read);
    assert_eq!(pages.pages, PermissionLevel::Write);
    assert_eq!(pages.id_token, PermissionLevel::Write);
}

#[test]
fn typed_artifact_graph_preserves_hidden_metadata_and_exact_outputs() {
    let graph = graph();
    let verify = &graph.ir.jobs["verify"];
    let stage = &graph.ir.jobs["stage"];
    let deploy = &graph.ir.jobs["deploy"];

    for (job, step_id, artifact_name, artifact_path) in [
        (
            verify,
            "incoming_upload",
            "apt-incoming-${{ github.run_id }}-${{ github.run_attempt }}",
            "incoming",
        ),
        (
            stage,
            "staging_upload",
            "apt-staging-${{ github.run_id }}-${{ github.run_attempt }}",
            "public",
        ),
    ] {
        let upload = job
            .steps
            .iter()
            .find(|step| step.id.as_ref().is_some_and(|id| id.as_str() == step_id))
            .expect("upload step");
        let StepKind::Action { uses, with, .. } = &upload.kind else {
            panic!("upload must be an action")
        };
        assert!(uses.starts_with("actions/upload-artifact@"));
        assert_eq!(with.get("name").map(String::as_str), Some(artifact_name));
        assert_eq!(with.get("path").map(String::as_str), Some(artifact_path));
        assert_eq!(
            with.get("include-hidden-files").map(String::as_str),
            Some("true")
        );
        assert_eq!(
            with.get("if-no-files-found").map(String::as_str),
            Some("error")
        );
        assert_eq!(with.get("retention-days").map(String::as_str), Some("2"));

        let id = job
            .outputs
            .iter()
            .find(|output| output.name == "artifact_id")
            .expect("artifact id output");
        let digest = job
            .outputs
            .iter()
            .find(|output| output.name == "artifact_digest")
            .expect("artifact digest output");
        assert_eq!(id.value.step_id, StepId::new(step_id).expect("step id"));
        assert_eq!(digest.value.step_id, id.value.step_id);
        assert_eq!(id.value.output, ActionOutput::ArtifactId);
        assert_eq!(digest.value.output, ActionOutput::ArtifactDigest);
    }

    assert_eq!(stage.needs, ["verify", "admit"]);
    assert_eq!(deploy.needs, ["stage", "admit"]);
    let transport =
        env(named_step(stage, "Verify and download feed artifact")).expect("transport env");
    assert_eq!(
        transport.get("ARTIFACT_ID").map(String::as_str),
        Some("${{ needs.verify.outputs.artifact_id }}")
    );
    assert_eq!(
        transport.get("ARTIFACT_DIGEST").map(String::as_str),
        Some("${{ needs.verify.outputs.artifact_digest }}")
    );
    assert!(!transport.keys().any(|key| key.starts_with("APT_GPG_")));
}

#[test]
fn typed_triggers_keep_schedule_choices_and_publication_guard_closed() {
    let graph = graph();
    let schedule = graph.ir.triggers.schedule.as_ref().expect("schedule");
    assert_eq!(schedule.cron, [config().schedule]);
    let dispatch = graph
        .ir
        .triggers
        .workflow_dispatch
        .as_ref()
        .expect("dispatch");
    let mode = input(&dispatch.inputs, "mode");
    assert_eq!(mode.input_type, DispatchInputType::Choice);
    assert_eq!(mode.options, ["validate", "publish"]);
    assert_eq!(mode.default.as_deref(), Some("validate"));
    let channel = input(&dispatch.inputs, "channel");
    assert_eq!(channel.input_type, DispatchInputType::Choice);
    assert_eq!(channel.options, ["stable", "preview"]);
    assert_eq!(channel.default.as_deref(), Some("stable"));
    assert_eq!(
        input(&dispatch.inputs, "version").default.as_deref(),
        Some("")
    );
    assert_eq!(
        input(&dispatch.inputs, "commit").default.as_deref(),
        Some("")
    );

    let condition = graph.ir.jobs["stage"]
        .condition
        .as_deref()
        .expect("stage guard");
    assert!(condition.contains("github.repository == 'tailrocks/velnor-apt'"));
    assert!(condition.contains("github.ref == 'refs/heads/main'"));
    assert!(condition.contains("github.event_name == 'schedule'"));
    assert!(condition.contains("inputs.mode == 'publish'"));
    assert_eq!(
        graph.ir.jobs["feed-result"].condition.as_deref(),
        Some("${{ always() }}")
    );
}

#[test]
fn typed_tool_preparation_is_qualified_before_native_helpers() {
    let graph = graph();
    assert!(graph.source_helpers.iter().any(|record| {
        record.invocation().descriptor().operation() == SourceBoundOperation::MiseBootstrap
    }));

    for id in ["verify", "admit", "stage", "deploy"] {
        let job = &graph.ir.jobs[id];
        assert!(matches!(
            job.steps.first().map(|step| &step.kind),
            Some(StepKind::Action { uses, .. }) if uses.starts_with("actions/checkout@")
        ));
        assert!(matches!(
            job.steps.get(1).map(|step| &step.kind),
            Some(StepKind::SourceBoundHelper { invocation, .. })
                if invocation.descriptor().operation() == SourceBoundOperation::MiseBootstrap
        ));
        let Some(StepKind::SourceBoundHelper { invocation, env }) =
            job.steps.get(2).map(|step| &step.kind)
        else {
            panic!("{id} lacks typed tool installation")
        };
        assert_eq!(
            invocation.descriptor().operation(),
            SourceBoundOperation::MiseToolPrepare
        );
        assert_eq!(
            invocation.args(),
            &["full", "gh@2.102.0", "python@3.14.7",].map(str::to_owned)
        );
        let installation = graph
            .source_helpers
            .iter()
            .find(|record| record.invocation() == invocation)
            .expect("registered tool installation");
        assert_eq!(env, installation.environment());
    }

    let buildx = graph.ir.jobs["verify"]
        .steps
        .iter()
        .find(|step| step.name == "Set up Buildx")
        .expect("Buildx step");
    let StepKind::Action { uses, with, .. } = &buildx.kind else {
        panic!("Buildx must be typed action")
    };
    assert_eq!(uses, &context().buildx_action);
    assert_eq!(with.get("version").map(String::as_str), Some("v0.37.2"));
    assert_eq!(with.get("cache-binary").map(String::as_str), Some("false"));
    assert_eq!(
        with.get("driver-opts").map(String::as_str),
        Some(format!("image={}", context().buildkit_image).as_str())
    );
}

#[test]
fn forged_artifact_pages_and_full_ci_bindings_fail_contract_validation() {
    let graph = graph();

    let mut artifact = graph.ir.clone();
    artifact.jobs.get_mut("verify").expect("verify").outputs[0]
        .value
        .step_id = StepId::new("forged").expect("id");
    assert!(artifact.validate().is_err());

    let mut pages = graph.ir.clone();
    pages
        .jobs
        .get_mut("deploy")
        .expect("deploy")
        .native_pages_deploy = None;
    assert!(pages.validate().is_err());

    let mut action = graph.ir.clone();
    action
        .jobs
        .get_mut("deploy")
        .expect("deploy")
        .native_pages_deploy
        .as_mut()
        .expect("Pages role")
        .actions
        .configure = format!("actions/configure-pages@{}", "0".repeat(40));
    assert!(action.validate().is_err());

    let mut full_ci = graph.ir;
    let full_ci_job = full_ci.jobs["deploy"]
        .native_pages_deploy
        .as_ref()
        .expect("Pages role")
        .full_ci_job
        .clone();
    full_ci
        .jobs
        .get_mut("deploy")
        .expect("deploy")
        .needs
        .retain(|need| need != &full_ci_job);
    assert!(full_ci.validate().is_err());
}

#[test]
fn unsafe_runner_and_buildx_bindings_fail_before_graph_emission() {
    let mut bad_runner = context();
    bad_runner.workflow.runs_on = "ubuntu-latest".to_owned();
    assert!(super::workflow(&config(), &bad_runner).is_err());

    let mut bad_buildx = context();
    bad_buildx.buildx_action = "docker/setup-buildx-action@latest".to_owned();
    assert!(super::workflow(&config(), &bad_buildx).is_err());
}

#[test]
fn self_consistent_forged_pages_action_cannot_replace_factory_approval() {
    use velnor_actions_workflow_renderer::{WorkflowDocumentContext, render_workflow_document};
    let graph = graph();
    let context = WorkflowDocumentContext {
        generator_version: "0.1.0".to_owned(),
        source_helpers: graph.source_helpers,
        native_pages_approvals: graph.pages_approvals,
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    let mut ir = graph.ir;
    let job = ir.jobs.get_mut("deploy").expect("deploy");
    let role = job.native_pages_deploy.as_mut().expect("role");
    let approved = role.actions.configure.clone();
    let forged = format!("actions/configure-pages@{}", "0".repeat(40));
    role.actions.configure = forged.clone();
    for step in &mut job.steps {
        if let StepKind::Action { uses, .. } = &mut step.kind
            && uses == &approved
        {
            *uses = forged.clone();
        }
    }
    ir.validate().expect("role is internally consistent");
    let error =
        render_workflow_document(&ir, &context).expect_err("compiled approval rejects forgery");
    assert!(error.to_string().contains("unapproved_native_pages_deploy"));
}

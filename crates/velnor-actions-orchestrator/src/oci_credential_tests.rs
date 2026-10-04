//! Generation-only renderer fixture; proves no SDK publication or runtime qualification.
use super::{OciRenderContext, credential_approvals, jobs, scripts, steps, tests};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CacheMode, CompiledNativeExecRecipe, CompiledSourceHelper, Concurrency, HelperInvocation,
    SourceBoundHelper, SourceBoundOperation, Step, StepId, StepKind, Trigger, WorkflowIr,
    workflow::native_tools::NativeCredentialScope,
};
use velnor_actions_workflow_renderer::{WorkflowDocumentContext, render_workflow_document};

fn proof_record(context: &OciRenderContext) -> CompiledSourceHelper {
    let source =
        velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("fixture source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::OciDelivery;
    let helper =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let invocation = HelperInvocation::compiled(
        helper,
        scripts::verify_script(
            &context.repository,
            &context.ci_workflow,
            &context.default_branch,
        ),
        Vec::new(),
    )
    .expect("invocation");
    let environment = BTreeMap::from([
        ("GH_TOKEN".into(), "${{ github.token }}".into()),
        ("EVENT_NAME".into(), "${{ github.event_name }}".into()),
        ("REF".into(), "${{ github.ref }}".into()),
        ("SOURCE_SHA".into(), "${{ github.sha }}".into()),
        ("REPOSITORY".into(), "${{ github.repository }}".into()),
    ]);
    let selectors = vec!["python@3.14.0".into()];
    let prefix = [
        "/usr/bin/env",
        "-i",
        "GH_TOKEN=${GH_TOKEN}",
        "/fixture/mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "python@3.14.0",
        "--",
    ]
    .map(str::to_owned)
    .to_vec();
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        prefix,
        BTreeMap::from([("GH_TOKEN".into(), "${{ github.token }}".into())]),
        selectors,
        NativeCredentialScope::GithubReadOnly,
    )
    .expect("neutral scope fixture");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("record")
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .expect("binding")
}

fn fixture() -> (WorkflowIr, WorkflowDocumentContext) {
    let context = tests::context();
    let config = tests::config();
    let record = proof_record(&context);
    let proof = Step {
        id: Some(StepId::new("verify").expect("id")),
        name: "Neutral source proof".into(),
        condition: None,
        kind: StepKind::SourceBoundHelper {
            invocation: record.invocation().clone(),
            env: record.environment().clone(),
        },
    };
    let mut verify = jobs::job("verify", Vec::new(), vec![proof]).expect("job");
    verify.condition = Some(format!(
        "success() && github.repository == '{}' && startsWith(github.ref, 'refs/tags/v') && (github.event_name == 'push' || github.event_name == 'workflow_dispatch')",
        context.repository
    ));
    let login = jobs::job(
        "admit-base",
        vec!["verify".into()],
        vec![steps::login(&config, &context).expect("login")],
    )
    .expect("job");
    let workflow = WorkflowIr {
        cache_mode: CacheMode::Read,
        name: "Neutral OCI approval fixture".into(),
        run_name: None,
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            push_tags: vec!["v[0-9]*".into()],
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: jobs::read_permissions(),
        concurrency: Concurrency {
            group: "oci".into(),
            cancel_in_progress: "false".into(),
        },
        jobs: BTreeMap::from([("verify".into(), verify), ("admit-base".into(), login)]),
    };
    let approvals =
        credential_approvals(&config, &context, &workflow).expect("compiled login wiring");
    let document = WorkflowDocumentContext {
        generator_version: "0.1.0".into(),
        source_helpers: vec![record],
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: approvals,
    };
    (workflow, document)
}

#[test]
fn isolated_credentials_wiring_renders_exact_named_login() {
    let (workflow, context) = fixture();
    let rendered = render_workflow_document(&workflow, &context).expect("neutral renderer fixture");
    assert!(rendered.contains("secrets.DOCKERHUB_USERNAME"));
    assert!(rendered.contains("secrets.DOCKERHUB_TOKEN"));
    assert_eq!(context.action_credential_approvals.len(), 1);
    let mut missing = context.clone();
    missing.action_credential_approvals.clear();
    assert!(render_workflow_document(&workflow, &missing).is_err());
}

#[derive(Clone, Copy)]
enum Mutation {
    Pin,
    UsernameSecret,
    PasswordSecret,
    DockerConfig,
    JobId,
    Needs,
    Ref,
    Source,
}

fn mutate(workflow: &mut WorkflowIr, mutation: Mutation) {
    if matches!(mutation, Mutation::JobId) {
        let job = workflow.jobs.remove("admit-base").expect("job");
        workflow.jobs.insert("admit-foreign".into(), job);
        return;
    }
    if matches!(mutation, Mutation::Needs) {
        workflow
            .jobs
            .get_mut("admit-base")
            .expect("job")
            .needs
            .clear();
        return;
    }
    if matches!(mutation, Mutation::Ref | Mutation::Source) {
        let StepKind::SourceBoundHelper { invocation, env } =
            &mut workflow.jobs.get_mut("verify").expect("proof").steps[0].kind
        else {
            panic!("helper");
        };
        if matches!(mutation, Mutation::Ref) {
            env.insert("REF".into(), "refs/heads/main".into());
        } else {
            let mut wire = serde_json::to_value(&*invocation).expect("wire");
            wire["helper"]["source_sha256"] = serde_json::json!("b".repeat(64));
            *invocation = serde_json::from_value(wire).expect("shape");
        }
        return;
    }
    let StepKind::Action { uses, with, env } =
        &mut workflow.jobs.get_mut("admit-base").expect("job").steps[0].kind
    else {
        panic!("login");
    };
    match mutation {
        Mutation::Pin => *uses = format!("docker/login-action@{}", "b".repeat(40)),
        Mutation::UsernameSecret => {
            with.insert("username".into(), "${{ secrets.OTHER }}".into());
        }
        Mutation::PasswordSecret => {
            with.insert("password".into(), "${{ secrets.OTHER }}".into());
        }
        Mutation::DockerConfig => {
            env.insert("DOCKER_CONFIG".into(), "${{ runner.temp }}/foreign".into());
        }
        Mutation::JobId | Mutation::Needs | Mutation::Ref | Mutation::Source => {
            unreachable!("handled")
        }
    }
}

#[test]
fn frozen_approval_rejects_source_graph_or_login_mutation() {
    let (workflow, context) = fixture();
    render_workflow_document(&workflow, &context).expect("baseline before mutations");
    for mutation in [
        Mutation::Pin,
        Mutation::UsernameSecret,
        Mutation::PasswordSecret,
        Mutation::DockerConfig,
        Mutation::JobId,
        Mutation::Needs,
        Mutation::Ref,
        Mutation::Source,
    ] {
        let mut changed = workflow.clone();
        mutate(&mut changed, mutation);
        assert!(render_workflow_document(&changed, &context).is_err());
    }
}

#[test]
fn archive_exporter_has_no_registry_write_and_preserves_attestations() {
    let context = tests::context();
    let config = tests::config();
    let image = &config.images[0];
    for arch in ["amd64", "arm64"] {
        let action = steps::build_action(&context, image, arch).expect("export action");
        let StepKind::Action { with, .. } = action.kind else {
            panic!("action");
        };
        assert_eq!(with["push"], "false");
        assert_eq!(
            with["outputs"],
            format!(
                "type=oci,dest=${{{{ runner.temp }}}}/velnor/oci-base-{arch}.tar,oci-mediatypes=true"
            )
        );
        assert_eq!(with["provenance"], "mode=max");
        assert!(with["sbom"].starts_with("generator=docker/buildkit-syft-scanner@sha256:"));
        assert_eq!(
            scripts::platform_publish_script(
                &context.repository,
                &context.ci_workflow,
                &context.default_branch
            ),
            [
                "platform-publish",
                &context.repository,
                &context.ci_workflow,
                "main"
            ]
        );
    }
}

use super::{ActionCredentialApproval, DockerRegistryLoginBinding};
use crate::support::check_native_token_hygiene;
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{
    CacheMode, CompiledNativeExecRecipe, CompiledSourceHelper, Concurrency, HelperInvocation, Job,
    JobTimeout, Permissions, SourceBoundHelper, SourceBoundOperation, Step, StepId, StepKind,
    Trigger, WorkflowIr,
};
const JOB_ID: &str = "publish";
const LOGIN_ID: &str = "registry_login";
const FULL_CI_JOB: &str = "verify";
const REPOSITORY: &str = "owner/repository";
const CI_WORKFLOW: &str = "ci.yml";
const DEFAULT_BRANCH: &str = "main";
const LOGIN_USES: &str = "docker/login-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const GITHUB_TOKEN: &str = "${{ github.token }}";
const DOCKER_CONFIG: &str = "${{ runner.temp }}/velnor/oci-docker";
struct Fixture {
    workflow: WorkflowIr,
    record: CompiledSourceHelper,
    binding: DockerRegistryLoginBinding,
}

fn fixture() -> Fixture {
    let record = helper_record(NativeCredentialScope::GithubReadOnly);
    let workflow = WorkflowIr {
        cache_mode: CacheMode::Read,
        name: "OCI delivery".to_owned(),
        run_name: None,
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            push_tags: vec!["v[0-9]*".to_owned()],
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "oci".to_owned(),
            cancel_in_progress: "false".to_owned(),
        },
        jobs: BTreeMap::from([
            (
                FULL_CI_JOB.to_owned(),
                job("Full CI proof", verify_step(&record)),
            ),
            (JOB_ID.to_owned(), job("Publish image", login_step())),
        ]),
    };
    Fixture {
        binding: binding(record.invocation().clone()),
        workflow,
        record,
    }
}

fn binding(full_ci_admission: HelperInvocation) -> DockerRegistryLoginBinding {
    DockerRegistryLoginBinding {
        approved_login_uses: LOGIN_USES.to_owned(),
        registry: "docker.io".to_owned(),
        username_secret: "DOCKERHUB_USERNAME".to_owned(),
        password_secret: "DOCKERHUB_TOKEN".to_owned(),
        repository: REPOSITORY.to_owned(),
        default_branch: DEFAULT_BRANCH.to_owned(),
        ci_workflow: CI_WORKFLOW.to_owned(),
        full_ci_job: FULL_CI_JOB.to_owned(),
        full_ci_admission,
    }
}

fn approve(
    binding: DockerRegistryLoginBinding,
    workflow: &WorkflowIr,
) -> Result<ActionCredentialApproval, crate::RenderError> {
    ActionCredentialApproval::docker_registry_login(
        JOB_ID,
        &StepId::new(LOGIN_ID).expect("login id"),
        &binding,
        workflow,
    )
}

fn job(display_name: &str, step: Step) -> Job {
    Job {
        cache_mode: None,
        display_name: display_name.to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::RELEASE,
        needs: (display_name == "Publish image")
            .then(|| vec![FULL_CI_JOB.to_owned()])
            .unwrap_or_default(),
        condition: (display_name == "Full CI proof").then(|| {
            format!(
                "success() && github.repository == '{REPOSITORY}' && startsWith(github.ref, 'refs/tags/v') && (github.event_name == 'push' || github.event_name == 'workflow_dispatch')"
            )
        }),
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps: vec![step],
    }
}

fn verify_step(record: &CompiledSourceHelper) -> Step {
    Step {
        id: Some(StepId::new("verify").expect("verify id")),
        name: "Verify source".to_owned(),
        condition: None,
        kind: StepKind::SourceBoundHelper {
            invocation: record.invocation().clone(),
            env: record.environment().clone(),
        },
    }
}

fn login_step() -> Step {
    Step {
        id: Some(StepId::new(LOGIN_ID).expect("login id")),
        name: "Registry login".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: LOGIN_USES.to_owned(),
            with: BTreeMap::from([
                ("registry".to_owned(), "docker.io".to_owned()),
                (
                    "username".to_owned(),
                    "${{ secrets.DOCKERHUB_USERNAME }}".to_owned(),
                ),
                (
                    "password".to_owned(),
                    "${{ secrets.DOCKERHUB_TOKEN }}".to_owned(),
                ),
            ]),
            env: BTreeMap::from([("DOCKER_CONFIG".to_owned(), DOCKER_CONFIG.to_owned())]),
        },
    }
}

fn helper_record(scope: NativeCredentialScope) -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::OciDelivery;
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let invocation = HelperInvocation::compiled(
        descriptor,
        vec![
            "verify".to_owned(),
            REPOSITORY.to_owned(),
            CI_WORKFLOW.to_owned(),
            DEFAULT_BRANCH.to_owned(),
        ],
        Vec::new(),
    )
    .expect("invocation");
    let selectors = vec!["python@3.14.0".to_owned()];
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        vec![
            "/usr/bin/env",
            "-i",
            "/owned/mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "python@3.14.0",
            "--",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        BTreeMap::from([("GH_TOKEN".to_owned(), GITHUB_TOKEN.to_owned())]),
        selectors,
        scope,
    )
    .expect("recipe");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("compiled source")
        .with_environment(source_environment())
        .with_execution_recipe(recipe)
        .expect("bound source")
}

fn source_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("GH_TOKEN".to_owned(), GITHUB_TOKEN.to_owned()),
        (
            "EVENT_NAME".to_owned(),
            "${{ github.event_name }}".to_owned(),
        ),
        ("REF".to_owned(), "${{ github.ref }}".to_owned()),
        ("SOURCE_SHA".to_owned(), "${{ github.sha }}".to_owned()),
        (
            "REPOSITORY".to_owned(),
            "${{ github.repository }}".to_owned(),
        ),
    ])
}

fn action_kind(workflow: &mut WorkflowIr) -> &mut StepKind {
    &mut workflow.jobs.get_mut(JOB_ID).expect("job").steps[0].kind
}

fn proof_step(workflow: &mut WorkflowIr) -> &mut Step {
    &mut workflow.jobs.get_mut(FULL_CI_JOB).expect("proof").steps[0]
}

fn tampered_invocation(invocation: &HelperInvocation) -> HelperInvocation {
    let mut wire = serde_json::to_value(invocation).expect("invocation");
    wire["helper"]["source_sha256"] = serde_json::json!("b".repeat(64));
    serde_json::from_value(wire).expect("shape")
}

fn assert_reuse_rejected<F>(fixture: &Fixture, approval: &ActionCredentialApproval, mutate: F)
where
    F: FnOnce(&mut WorkflowIr),
{
    let mut changed = fixture.workflow.clone();
    mutate(&mut changed);
    let id = StepId::new(LOGIN_ID).expect("login id");
    assert!(!approval.admits(JOB_ID, Some(&id), &changed));
    assert!(
        check_native_token_hygiene(
            &changed,
            std::slice::from_ref(&fixture.record),
            std::slice::from_ref(approval),
        )
        .is_err()
    );
}

fn gate_ok(
    workflow: &WorkflowIr,
    record: &CompiledSourceHelper,
    approvals: &[ActionCredentialApproval],
) -> bool {
    check_native_token_hygiene(workflow, std::slice::from_ref(record), approvals).is_ok()
}

#[test]
fn approval_and_native_token_gate_admit_one_exact_login() {
    let fixture = fixture();
    let approval = approve(fixture.binding.clone(), &fixture.workflow).expect("approval");
    let id = StepId::new(LOGIN_ID).expect("login id");
    assert!(approval.admits(JOB_ID, Some(&id), &fixture.workflow));
    assert!(gate_ok(
        &fixture.workflow,
        &fixture.record,
        &[approval.clone()]
    ));
    assert!(!gate_ok(&fixture.workflow, &fixture.record, &[]));
    assert!(!gate_ok(
        &fixture.workflow,
        &fixture.record,
        &[approval.clone(), approval.clone()],
    ));
    assert!(!gate_ok(
        &fixture.workflow,
        &helper_record(NativeCredentialScope::OciRegistryPublish),
        &[approval],
    ));
}

#[test]
fn approval_reuse_rejects_graph_pin_secret_env_needs_and_step_condition() {
    let fixture = fixture();
    let approval = approve(fixture.binding.clone(), &fixture.workflow).expect("approval");
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        workflow.name = "foreign".to_owned()
    });
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        let StepKind::Action { uses, .. } = action_kind(workflow) else {
            panic!("login")
        };
        *uses = "docker/login-action@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    });
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        let StepKind::Action { with, .. } = action_kind(workflow) else {
            panic!("login")
        };
        with.insert("password".to_owned(), "${{ secrets.OTHER }}".to_owned());
    });
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        let StepKind::Action { env, .. } = action_kind(workflow) else {
            panic!("login")
        };
        env.insert("DOCKER_CONFIG".to_owned(), "foreign".to_owned());
    });
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        workflow.jobs.get_mut(JOB_ID).expect("job").needs.clear();
    });
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        workflow.jobs.get_mut(JOB_ID).expect("job").steps[0].condition =
            Some("always()".to_owned());
    });
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        if let StepKind::SourceBoundHelper { env, .. } = &mut proof_step(workflow).kind {
            env.insert("REF".to_owned(), "refs/heads/main".to_owned());
        }
    });
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        let step = proof_step(workflow);
        let (invocation, env) = match &step.kind {
            StepKind::SourceBoundHelper { invocation, env } => {
                (tampered_invocation(invocation), env.clone())
            }
            _ => panic!("proof"),
        };
        step.kind = StepKind::SourceBoundHelper { invocation, env };
    });
    assert_reuse_rejected(&fixture, &approval, |workflow| {
        workflow.jobs.get_mut(FULL_CI_JOB).expect("proof").condition = Some("always()".to_owned());
    });
}

#[test]
fn constructor_rejects_malformed_bindings_and_proof_shapes() {
    let fixture = fixture();
    let rejected = |binding: DockerRegistryLoginBinding, workflow: &WorkflowIr| {
        approve(binding, workflow).is_err()
    };
    for pin in [
        "docker/login-action@main",
        "docker/login-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "actions/login@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let mut binding = fixture.binding.clone();
        binding.approved_login_uses = pin.to_owned();
        assert!(rejected(binding, &fixture.workflow));
    }
    for secret in ["", "1BAD", "BAD-NAME", "GITHUB_TOKEN"] {
        let mut binding = fixture.binding.clone();
        binding.username_secret = secret.to_owned();
        assert!(rejected(binding, &fixture.workflow), "{secret:?}");
    }
    let mut binding = fixture.binding.clone();
    binding.registry = "ghcr.io".to_owned();
    assert!(rejected(binding, &fixture.workflow));
    let mut binding = fixture.binding.clone();
    binding.repository = "owner/foreign".to_owned();
    assert!(rejected(binding, &fixture.workflow));
    let mut binding = fixture.binding.clone();
    binding.full_ci_job = "missing".to_owned();
    assert!(rejected(binding, &fixture.workflow));
    let mut binding = fixture.binding.clone();
    let mut wire = serde_json::to_value(&binding.full_ci_admission).expect("invocation");
    wire["args"] = serde_json::json!(["verify", REPOSITORY, "foreign.yml", DEFAULT_BRANCH]);
    binding.full_ci_admission = serde_json::from_value(wire).expect("shape");
    assert!(rejected(binding, &fixture.workflow));
    let mut docker_proof = fixture.workflow.clone();
    if let StepKind::SourceBoundHelper { env, .. } = &mut proof_step(&mut docker_proof).kind {
        env.insert("DOCKER_CONFIG".to_owned(), DOCKER_CONFIG.to_owned());
    }
    assert!(approve(fixture.binding.clone(), &docker_proof).is_err());
    let mut missing_needs = fixture.workflow.clone();
    missing_needs
        .jobs
        .get_mut(JOB_ID)
        .expect("job")
        .needs
        .clear();
    assert!(approve(fixture.binding.clone(), &missing_needs).is_err());
}

#[test]
fn constructor_accepts_only_successful_dependency_guards() {
    let fixture = fixture();
    for condition in [
        Some("needs.verify.result == 'success' && github.ref == 'refs/tags/v1'"),
        Some(
            "always() && needs.verify.result == 'success' && (github.event_name == 'push' || github.event_name == 'workflow_dispatch')",
        ),
        None,
    ] {
        let mut workflow = fixture.workflow.clone();
        workflow.jobs.get_mut(JOB_ID).expect("job").condition = condition.map(str::to_owned);
        assert!(approve(fixture.binding.clone(), &workflow).is_ok());
    }
    for condition in [
        "always()",
        "failure() && needs.verify.result == 'success'",
        "needs.verify.result == 'success' || true",
        "needs.verify.result == 'success' && (github.ref == 'refs/tags/v1'",
        "needs.verify.result == 'success' && github.ref == 'refs/tags/v1' && 'unterminated",
    ] {
        let mut workflow = fixture.workflow.clone();
        workflow.jobs.get_mut(JOB_ID).expect("job").condition = Some(condition.to_owned());
        assert!(approve(fixture.binding.clone(), &workflow).is_err());
    }
}

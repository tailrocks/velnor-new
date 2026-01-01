//! Release step-content gate cases (authority separation, argv safety).
use std::collections::BTreeMap;
use velnor_actions_contract_workflow::{Step, StepKind};
use velnor_actions_workflow_release::release_gates::{ReleaseConfigBinding, check_release_jobs};
use velnor_actions_workflow_release::release_jobs::{
    ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
};
use velnor_actions_workflow_release::release_permissions::JobPermissions;
use velnor_actions_workflow_release::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseConcurrency, ReleaseTriggers, publish_gate_condition,
};
use velnor_actions_workflow_release::release_tree::{
    RELEASE_BOOTSTRAP_CONFIG_PATH, RELEASE_CONFIG_PATH,
};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_steps::action_step;

pub(crate) const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
pub(crate) const OTHER_SHA: &str = "abcdef0123456789abcdef0123456789abcdef01";
pub(crate) const REPO: &str = "acme/widgets";
pub(crate) const LABEL: &str = "ubuntu-24.04";
pub(crate) const ENV: &str = "crates-io";

/// Extract the `InvalidWorkflow` payload; `None` unless the exact rejection fired.
pub(crate) fn invalid(result: Result<(), RenderError>) -> Option<String> {
    match result {
        Err(RenderError::InvalidWorkflow(text)) => Some(text),
        Err(_) | Ok(()) => None,
    }
}

pub(crate) fn checkout_uses() -> String {
    format!("actions/checkout@{:040x}", 0)
}

/// Exact forge-token env binding every release-plz step carries.
pub(crate) const FORGE_ENV: [(&str, &str); 1] = [("GIT_TOKEN", "${{ secrets.GITHUB_TOKEN }}")];

/// Raw checkout step over an explicit input map (negative cases).
pub(crate) fn checkout(with: &[(&str, &str)]) -> Result<Step, RenderError> {
    action_step(
        "Checkout",
        &checkout_uses(),
        with.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    )
}

/// Policy checkout: event commit, full history, explicit credentials.
pub(crate) fn policy_checkout(persist: Option<&str>) -> Result<Step, RenderError> {
    let mut inputs = vec![];
    if let Some(value) = persist {
        inputs.push(("persist-credentials", value));
    }
    inputs.push(("fetch-depth", "0"));
    checkout(&inputs)
}

/// Source checkout: approved SHA pin under the fixed path, full history.
pub(crate) fn source_checkout(sha: &str, persist: Option<&str>) -> Result<Step, RenderError> {
    let mut inputs = vec![];
    if let Some(value) = persist {
        inputs.push(("persist-credentials", value));
    }
    inputs.extend([
        ("fetch-depth", "0"),
        ("path", "release-source"),
        ("ref", sha),
    ]);
    checkout(&inputs)
}

pub(crate) fn shell(name: &str, argv: &[&str], env: &[(&str, &str)]) -> Step {
    // Direct IR: these tests pin the release gates as the enforcing
    // layer, so construction bypasses the constructor gates.
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: argv.iter().map(ToString::to_string).collect(),
            env: env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        },
    }
}

pub(crate) fn publish_argv(config: &str) -> Vec<&str> {
    vec!["release-plz", "release", "--config", config]
}

pub(crate) fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: "plan-1".to_owned(),
        repository: REPO.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates_io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
        version: None,
    }
}

pub(crate) fn job(
    role: ReleaseRole,
    steps: Vec<Step>,
    condition: Option<&str>,
    env: Option<&str>,
) -> ReleaseJobSpec {
    ReleaseJobSpec {
        role,
        display_name: format!("Release {}", role.as_str()),
        runs_on: LABEL.to_owned(),
        timeout_minutes: velnor_actions_contract_workflow::JobTimeout::RELEASE,
        needs: Vec::new(),
        condition: condition.map(str::to_owned),
        environment: env.map(str::to_owned),
        permissions: JobPermissions::expected(role),
        steps,
    }
}

pub(crate) fn spec(jobs: BTreeMap<String, ReleaseJobSpec>) -> ReleaseWorkflowSpec {
    ReleaseWorkflowSpec {
        name: "Velnor Release".to_owned(),
        repository: REPO.to_owned(),
        triggers: ReleaseTriggers {
            push_branches: vec!["main".to_owned()],
            schedule: None,
            dispatch_inputs: vec![
                DispatchInput {
                    name: "plan".to_owned(),
                    description: "approved plan".to_owned(),
                    required: true,
                    default: Some("plan-1".to_owned()),
                },
                DispatchInput {
                    name: "source_sha".to_owned(),
                    description: "approved source".to_owned(),
                    required: true,
                    default: Some(SHA.to_owned()),
                },
            ],
        },
        concurrency: ReleaseConcurrency {
            group: "release-acme/widgets".to_owned(),
            cancel_in_progress: false,
        },
        jobs,
        bootstrap: bootstrap(),
        publish_environment: ENV.to_owned(),
        bootstrap_environment: "crates-io-bootstrap".to_owned(),
    }
}

pub(crate) fn binding() -> ReleaseConfigBinding<'static> {
    ReleaseConfigBinding {
        effective: RELEASE_CONFIG_PATH,
        bootstrap: RELEASE_BOOTSTRAP_CONFIG_PATH,
    }
}

pub(crate) fn gated_spec() -> Result<ReleaseWorkflowSpec, RenderError> {
    let gate = publish_gate_condition(REPO, &bootstrap());
    Ok(spec(BTreeMap::from([
        (
            "release-preparation".to_owned(),
            job(
                ReleaseRole::Preparation,
                vec![shell("Run", &["echo", "ok"], &[])],
                None,
                None,
            ),
        ),
        (
            "release-preflight".to_owned(),
            job(
                ReleaseRole::Preflight,
                vec![
                    policy_checkout(Some("false"))?,
                    source_checkout(SHA, Some("false"))?,
                    shell("Run", &["echo", "ok"], &[]),
                ],
                None,
                None,
            ),
        ),
        (
            "release-publish".to_owned(),
            job(
                ReleaseRole::PublishOidc,
                vec![
                    policy_checkout(Some("false"))?,
                    source_checkout(SHA, Some("true"))?,
                    shell("Publish", &publish_argv(RELEASE_CONFIG_PATH), &FORGE_ENV),
                ],
                Some(&gate),
                Some(ENV),
            ),
        ),
        (
            "release-reconcile".to_owned(),
            job(
                ReleaseRole::Reconcile,
                vec![shell("Run", &["echo", "ok"], &[])],
                Some("always()"),
                None,
            ),
        ),
    ])))
}

pub(crate) fn job_steps(spec: &ReleaseWorkflowSpec, id: &str) -> Option<Vec<Step>> {
    spec.jobs.get(id).map(|job| job.steps.clone())
}

pub(crate) fn with_steps(
    mut spec: ReleaseWorkflowSpec,
    id: &str,
    steps: Vec<Step>,
) -> Option<ReleaseWorkflowSpec> {
    match spec.jobs.get_mut(id) {
        Some(job) => {
            job.steps = steps;
            Some(spec)
        }
        None => None,
    }
}

#[test]
fn preparation_forbids_secrets_tokens_and_inputs() -> Result<(), RenderError> {
    assert!(check_release_jobs(&gated_spec()?, &binding()).is_ok());
    let leaked = with_steps(
        gated_spec()?,
        "release-preparation",
        vec![shell("Run", &["echo", "${{ secrets.TOKEN }}"], &[])],
    )
    .expect("job");
    assert!(
        invalid(check_release_jobs(&leaked, &binding()))
            .expect("reject")
            .starts_with("secret_outside_bootstrap:")
    );
    let tokened = with_steps(
        gated_spec()?,
        "release-preparation",
        vec![shell(
            "Run",
            &["echo", "ok"],
            &[("CARGO_REGISTRY_TOKEN", "x")],
        )],
    )
    .expect("job");
    assert!(
        invalid(check_release_jobs(&tokened, &binding()))
            .expect("reject")
            .starts_with("registry_token_outside_bootstrap:")
    );
    let interpolated = with_steps(
        gated_spec()?,
        "release-preparation",
        vec![shell("Run", &["echo", "github.event.inputs.plan"], &[])],
    )
    .expect("job");
    assert!(
        invalid(check_release_jobs(&interpolated, &binding()))
            .expect("reject")
            .starts_with("dispatch_input_in_steps:")
    );
    Ok(())
}

#[test]
fn preflight_requires_the_exact_approved_source() -> Result<(), RenderError> {
    assert!(check_release_jobs(&gated_spec()?, &binding()).is_ok());
    for sha in [OTHER_SHA, "main", "v1.2.3"] {
        let rebound = with_steps(
            gated_spec()?,
            "release-preflight",
            vec![
                policy_checkout(Some("false"))?,
                source_checkout(sha, Some("false"))?,
                shell("Run", &["echo", "ok"], &[]),
            ],
        )
        .expect("job");
        assert_eq!(
            invalid(check_release_jobs(&rebound, &binding())).expect("reject"),
            "source_ref_mismatch:release-preflight",
            "for ref {sha}"
        );
    }
    let missing = with_steps(
        gated_spec()?,
        "release-preflight",
        vec![
            policy_checkout(Some("false"))?,
            shell("Run", &["echo", "ok"], &[]),
        ],
    )
    .expect("job");
    assert_eq!(
        invalid(check_release_jobs(&missing, &binding())).expect("reject"),
        "checkout_without_exact_source:release-preflight"
    );
    Ok(())
}

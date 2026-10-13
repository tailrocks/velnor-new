//! Release step-content gates: authority separation in every job.
//!
//! Proves no secrets outside the bootstrap env binding, no registry
//! token on the OIDC path, exact-source checkouts, explicit config
//! binding, verified Cargo, dispatch inputs confined to `if:`, and
//! serial publishing (no matrix fan-out, no internal ops).
use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::release_gates::{ReleaseConfigBinding, check_release_jobs};
use velnor_actions_workflow_renderer::release_jobs::{
    JobPermissions, ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
};
use velnor_actions_workflow_renderer::release_spec::lock::stable_lock_group;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseTriggers, publish_gate_condition,
};
use velnor_actions_workflow_renderer::release_tree::{
    RELEASE_BOOTSTRAP_CONFIG_PATH, RELEASE_CONFIG_PATH,
};
use velnor_actions_workflow_renderer::{RenderError, action_step, shell_step};

const REPOSITORY: &str = "acme/widgets";
const PLAN_ID: &str = "plan-1";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const LABEL: &str = "ubuntu-26.04";
const CHECKOUT: &str = "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683";

fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: PLAN_ID.to_owned(),
        repository: REPOSITORY.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates-io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
    }
}

fn triggers() -> ReleaseTriggers {
    let input = |name: &str, default: &str| DispatchInput {
        name: name.to_owned(),
        description: format!("Approved {name} reference"),
        required: true,
        default: Some(default.to_owned()),
    };
    ReleaseTriggers {
        push_branches: vec!["main".to_owned()],
        schedule: None,
        dispatch_inputs: vec![input("plan", PLAN_ID), input("source_sha", SHA)],
    }
}

fn checkout(exact: bool) -> Result<Step, RenderError> {
    let mut with = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    if exact {
        with.insert("ref".to_owned(), SHA.to_owned());
    }
    action_step("Checkout", CHECKOUT, with)
}

fn publish_argv(config: &str) -> Vec<String> {
    [
        "mise",
        "exec",
        "--",
        "release-plz",
        "release",
        "--config",
        config,
    ]
    .iter()
    .map(ToString::to_string)
    .collect()
}

fn shell(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    shell_step(name, argv, env)
}

fn plain_shell(name: &str) -> Result<Step, RenderError> {
    shell(
        name,
        vec!["mise".to_owned(), "exec".to_owned()],
        BTreeMap::new(),
    )
}

fn job(
    role: ReleaseRole,
    needs: &[&str],
    condition: Option<String>,
    steps: Vec<Step>,
) -> ReleaseJobSpec {
    let environment = match role {
        ReleaseRole::PublishOidc => Some("crates-io".to_owned()),
        ReleaseRole::PublishBootstrap => Some("crates-io-bootstrap".to_owned()),
        _ => None,
    };
    ReleaseJobSpec {
        role,
        display_name: format!("Release {}", role.as_str()),
        runs_on: LABEL.to_owned(),
        needs: needs.iter().map(ToString::to_string).collect(),
        condition,
        environment,
        permissions: JobPermissions::expected(role),
        steps,
    }
}

fn base_jobs(gate: &str) -> Result<BTreeMap<String, ReleaseJobSpec>, RenderError> {
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "preparation".to_owned(),
        job(
            ReleaseRole::Preparation,
            &[],
            None,
            vec![checkout(false)?, plain_shell("Prepare PR")?],
        ),
    );
    jobs.insert(
        "preflight".to_owned(),
        job(
            ReleaseRole::Preflight,
            &["preparation"],
            None,
            vec![checkout(true)?, plain_shell("Validate")?],
        ),
    );
    jobs.insert(
        "publish".to_owned(),
        job(
            ReleaseRole::PublishOidc,
            &["preflight"],
            Some(gate.to_owned()),
            vec![
                checkout(true)?,
                shell(
                    "Publish",
                    publish_argv(RELEASE_CONFIG_PATH),
                    BTreeMap::new(),
                )?,
            ],
        ),
    );
    Ok(jobs)
}

fn spec(with_bootstrap: bool) -> Result<ReleaseWorkflowSpec, RenderError> {
    let gate = publish_gate_condition(REPOSITORY, &bootstrap());
    let mut jobs = base_jobs(&gate)?;
    if with_bootstrap {
        jobs.insert(
            "publish-bootstrap".to_owned(),
            job(
                ReleaseRole::PublishBootstrap,
                &["preflight"],
                Some(gate.clone()),
                vec![
                    checkout(true)?,
                    shell(
                        "Bootstrap publish",
                        publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH),
                        BTreeMap::from([(
                            "CARGO_REGISTRY_TOKEN".to_owned(),
                            "${{ secrets.CRATES_TOKEN }}".to_owned(),
                        )]),
                    )?,
                ],
            ),
        );
    }
    let reconcile_needs: Vec<&str> = if with_bootstrap {
        vec!["publish", "publish-bootstrap"]
    } else {
        vec!["publish"]
    };
    jobs.insert(
        "reconcile".to_owned(),
        job(
            ReleaseRole::Reconcile,
            &reconcile_needs,
            Some("always()".to_owned()),
            vec![checkout(false)?, plain_shell("Reconcile")?],
        ),
    );
    Ok(ReleaseWorkflowSpec {
        name: "Release".to_owned(),
        repository: REPOSITORY.to_owned(),
        triggers: triggers(),
        concurrency: stable_lock_group("crates-io", REPOSITORY, "root")?,
        jobs,
        bootstrap: bootstrap(),
        publish_environment: "crates-io".to_owned(),
        bootstrap_environment: "crates-io-bootstrap".to_owned(),
    })
}

fn binding() -> ReleaseConfigBinding<'static> {
    ReleaseConfigBinding {
        effective: RELEASE_CONFIG_PATH,
        bootstrap: RELEASE_BOOTSTRAP_CONFIG_PATH,
    }
}

fn problem_of(result: Result<(), RenderError>) -> Result<String, RenderError> {
    match result {
        Err(RenderError::InvalidWorkflow(problem)) => Ok(problem),
        Err(other) => Err(other),
        Ok(()) => Err(RenderError::InvalidWorkflow(
            "unexpectedly_valid".to_owned(),
        )),
    }
}

fn check(spec: &ReleaseWorkflowSpec) -> Result<(), RenderError> {
    check_release_jobs(spec, &binding())
}

#[test]
fn gated_steps_pass_and_validate() -> Result<(), RenderError> {
    assert!(spec(false)?.validate().is_ok());
    assert!(spec(true)?.validate().is_ok());
    assert!(check(&spec(false)?).is_ok());
    assert!(check(&spec(true)?).is_ok());
    Ok(())
}

#[test]
fn validation_roles_reject_secrets_and_tokens() -> Result<(), RenderError> {
    for role_job in ["preparation", "preflight", "reconcile"] {
        let mut bad = spec(false)?;
        let steps = &mut bad.jobs.get_mut(role_job).expect("job").steps;
        steps.push(shell(
            "Leak",
            vec!["mise".to_owned(), "exec".to_owned()],
            BTreeMap::from([("FROM".to_owned(), "${{ secrets.X }}".to_owned())]),
        )?);
        let problem = problem_of(check(&bad))?;
        assert!(
            problem.contains("secret_outside_bootstrap"),
            "{role_job}: {problem}"
        );
    }
    let mut bad = spec(false)?;
    bad.jobs
        .get_mut("preflight")
        .expect("preflight")
        .steps
        .push(shell(
            "Token",
            vec!["mise".to_owned(), "exec".to_owned()],
            BTreeMap::from([("CARGO_REGISTRY_TOKEN".to_owned(), "x".to_owned())]),
        )?);
    assert!(problem_of(check(&bad))?.contains("registry_token_outside_bootstrap"));
    Ok(())
}

#[test]
fn preflight_and_publish_require_exact_checkout() -> Result<(), RenderError> {
    let mut bad = spec(false)?;
    bad.jobs.get_mut("preflight").expect("job").steps.remove(0);
    assert!(problem_of(check(&bad))?.contains("checkout_without_exact_source"));
    bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("job").steps.remove(0);
    assert!(problem_of(check(&bad))?.contains("checkout_without_exact_source"));
    Ok(())
}

#[test]
fn oidc_publish_rejects_any_secret_or_token() -> Result<(), RenderError> {
    let mut bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("job").steps.push(shell(
        "Leak",
        vec!["mise".to_owned(), "exec".to_owned()],
        BTreeMap::from([("FROM".to_owned(), "${{ secrets.X }}".to_owned())]),
    )?);
    assert!(problem_of(check(&bad))?.contains("secret_outside_bootstrap"));
    bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("job").steps.push(shell(
        "Token",
        vec!["mise".to_owned(), "exec".to_owned()],
        BTreeMap::from([("CARGO_REGISTRY_TOKEN".to_owned(), "x".to_owned())]),
    )?);
    assert!(problem_of(check(&bad))?.contains("registry_token_outside_bootstrap"));
    Ok(())
}

#[test]
fn publish_requires_config_binding_and_verification() -> Result<(), RenderError> {
    let mut bad = spec(false)?;
    let publish = bad.jobs.get_mut("publish").expect("job");
    publish.steps.pop();
    publish.steps.push(shell(
        "Publish",
        vec!["mise".to_owned(), "exec".to_owned()],
        BTreeMap::new(),
    )?);
    assert!(problem_of(check(&bad))?.contains("missing_config_binding"));
    for bypass in ["--allow-dirty", "--no-verify"] {
        bad = spec(false)?;
        let mut argv = publish_argv(RELEASE_CONFIG_PATH);
        argv.push(bypass.to_owned());
        let publish = bad.jobs.get_mut("publish").expect("job");
        publish.steps.pop();
        publish.steps.push(shell("Publish", argv, BTreeMap::new())?);
        let problem = problem_of(check(&bad))?;
        assert!(problem.contains("verify_bypass"), "{bypass}: {problem}");
    }
    Ok(())
}

#[test]
fn bootstrap_token_binding_is_exactly_once_env_only() -> Result<(), RenderError> {
    let mut bad = spec(true)?;
    let job = bad.jobs.get_mut("publish-bootstrap").expect("job");
    job.steps.pop();
    job.steps.push(shell(
        "Bootstrap publish",
        publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH),
        BTreeMap::new(),
    )?);
    assert!(problem_of(check(&bad))?.contains("bootstrap_token_binding"));
    bad = spec(true)?;
    bad.jobs
        .get_mut("publish-bootstrap")
        .expect("job")
        .steps
        .push(shell(
            "Argv leak",
            vec!["echo".to_owned(), "${{ secrets.X }}".to_owned()],
            BTreeMap::new(),
        )?);
    assert!(problem_of(check(&bad))?.contains("secret_in_argv"));
    Ok(())
}

#[test]
fn dispatch_inputs_never_interpolate_into_steps() -> Result<(), RenderError> {
    let mut bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("job").steps.push(shell(
        "Interpolate",
        vec!["echo".to_owned(), "github.event.inputs.plan".to_owned()],
        BTreeMap::new(),
    )?);
    assert!(problem_of(check(&bad))?.contains("dispatch_input_in_steps"));
    Ok(())
}

#[test]
fn checkout_hygiene_matrix_and_internal_rejected() -> Result<(), RenderError> {
    let mut bad = spec(false)?;
    let bare = action_step("Checkout", CHECKOUT, BTreeMap::new())?;
    bad.jobs.get_mut("preflight").expect("job").steps[0] = bare;
    assert!(problem_of(check(&bad))?.contains("checkout_with_credentials"));
    bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("job").steps.push(shell(
        "Fanout",
        vec!["mise".to_owned(), "exec".to_owned()],
        BTreeMap::from([("VELNOR_MATRIX_MAX_PARALLEL".to_owned(), "4".to_owned())]),
    )?);
    assert!(problem_of(check(&bad))?.contains("release_matrix_fanout"));
    bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("job").steps.push(Step {
        name: "Internal".to_owned(),
        kind: StepKind::Internal {
            operation: "x".to_owned(),
        },
    });
    assert!(problem_of(check(&bad))?.contains("release_internal_op"));
    Ok(())
}

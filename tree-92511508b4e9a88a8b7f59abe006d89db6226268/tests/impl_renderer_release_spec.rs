//! Release IR security contracts: permissions, lock, events, dispatch.
//!
//! Covers the workflow-level gates: the per-role least-privilege matrix
//! with a single OIDC writer, protected environment/branch enforcement,
//! fork and prohibited-event rejection, the stable publisher lock, and
//! approved-plan dispatch binding. Step-content gates live in the
//! companion `impl_renderer_release_tree` module.
use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::release_jobs::{
    JobPermissions, PermissionLevel, ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec,
    check_single_oidc_writer,
};
use velnor_actions_workflow_renderer::release_spec::lock::stable_lock_group;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseConcurrency, ReleaseTriggers, publish_gate_condition,
    validate_environment, validate_repository,
};
use velnor_actions_workflow_renderer::{RenderError, shell_step};

const REPOSITORY: &str = "acme/widgets";
const PLAN_ID: &str = "plan-1";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const LABEL: &str = "ubuntu-26.04";
const ENV_OIDC: &str = "crates-io";
const ENV_BOOTSTRAP: &str = "crates-io-bootstrap";

fn bootstrap() -> BootstrapPlan {
    BootstrapPlan {
        plan_id: PLAN_ID.to_owned(),
        repository: REPOSITORY.to_owned(),
        source_sha: SHA.to_owned(),
        registry: "crates-io".to_owned(),
        packages: BTreeMap::from([("widgets".to_owned(), "1.2.3".to_owned())]),
    }
}

fn dispatch(name: &str, default: &str) -> DispatchInput {
    DispatchInput {
        name: name.to_owned(),
        description: format!("Approved {name} reference"),
        required: true,
        default: Some(default.to_owned()),
    }
}

fn triggers() -> ReleaseTriggers {
    ReleaseTriggers {
        push_branches: vec!["main".to_owned()],
        schedule: None,
        dispatch_inputs: vec![dispatch("plan", PLAN_ID), dispatch("source_sha", SHA)],
    }
}

fn lock() -> Result<ReleaseConcurrency, RenderError> {
    stable_lock_group("crates-io", REPOSITORY, "root")
}

fn shell(name: &str) -> Result<Step, RenderError> {
    shell_step(
        name,
        vec!["mise".to_owned(), "exec".to_owned()],
        BTreeMap::new(),
    )
}

fn job(
    role: ReleaseRole,
    needs: &[&str],
    condition: Option<String>,
) -> Result<ReleaseJobSpec, RenderError> {
    let environment = match role {
        ReleaseRole::PublishOidc => Some(ENV_OIDC.to_owned()),
        ReleaseRole::PublishBootstrap => Some(ENV_BOOTSTRAP.to_owned()),
        _ => None,
    };
    Ok(ReleaseJobSpec {
        role,
        display_name: format!("Release {}", role.as_str()),
        runs_on: LABEL.to_owned(),
        needs: needs.iter().map(ToString::to_string).collect(),
        condition,
        environment,
        permissions: JobPermissions::expected(role),
        steps: vec![shell("Do work")?],
    })
}

fn jobs(with_bootstrap: bool) -> Result<BTreeMap<String, ReleaseJobSpec>, RenderError> {
    let gate = publish_gate_condition(REPOSITORY, &bootstrap());
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "preparation".to_owned(),
        job(ReleaseRole::Preparation, &[], None)?,
    );
    jobs.insert(
        "preflight".to_owned(),
        job(ReleaseRole::Preflight, &["preparation"], None)?,
    );
    jobs.insert(
        "publish".to_owned(),
        job(ReleaseRole::PublishOidc, &["preflight"], Some(gate.clone()))?,
    );
    if with_bootstrap {
        jobs.insert(
            "publish-bootstrap".to_owned(),
            job(
                ReleaseRole::PublishBootstrap,
                &["preflight"],
                Some(gate.clone()),
            )?,
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
        )?,
    );
    Ok(jobs)
}

fn spec(with_bootstrap: bool) -> Result<ReleaseWorkflowSpec, RenderError> {
    Ok(ReleaseWorkflowSpec {
        name: "Release".to_owned(),
        repository: REPOSITORY.to_owned(),
        triggers: triggers(),
        concurrency: lock()?,
        jobs: jobs(with_bootstrap)?,
        bootstrap: bootstrap(),
        publish_environment: ENV_OIDC.to_owned(),
        bootstrap_environment: ENV_BOOTSTRAP.to_owned(),
    })
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

#[test]
fn permission_matrix_grants_oidc_only_writer() {
    for role in [
        ReleaseRole::Preparation,
        ReleaseRole::Preflight,
        ReleaseRole::PublishOidc,
        ReleaseRole::PublishBootstrap,
        ReleaseRole::Reconcile,
    ] {
        let granted = JobPermissions::expected(role);
        let oidc = role == ReleaseRole::PublishOidc;
        assert_eq!(
            granted.id_token == PermissionLevel::Write,
            oidc,
            "id-token writer must be OIDC only: {}",
            role.as_str()
        );
        assert!(
            granted.validate(role, Some(ENV_OIDC)).is_ok(),
            "{} must validate",
            role.as_str()
        );
    }
    assert_eq!(
        JobPermissions::expected(ReleaseRole::PublishOidc).contents,
        PermissionLevel::Write
    );
    assert_eq!(
        JobPermissions::expected(ReleaseRole::Preflight).contents,
        PermissionLevel::Read
    );
}

#[test]
fn permission_validate_rejects_drift() -> Result<(), RenderError> {
    let foreign = JobPermissions::expected(ReleaseRole::Reconcile);
    let problem = problem_of(foreign.validate(ReleaseRole::Preflight, None))?;
    assert!(problem.contains("permission_matrix"), "{problem}");
    let oidc = JobPermissions::expected(ReleaseRole::PublishOidc);
    let problem = problem_of(oidc.validate(ReleaseRole::PublishOidc, None))?;
    assert!(
        problem.contains("id_token_without_environment"),
        "{problem}"
    );
    let mut validation = JobPermissions::expected(ReleaseRole::Preflight);
    validation.contents = PermissionLevel::Write;
    let problem = problem_of(validation.validate(ReleaseRole::Preflight, None))?;
    assert!(
        problem.contains("contents_write_on_validation"),
        "{problem}"
    );
    Ok(())
}

#[test]
fn single_oidc_writer_is_global() -> Result<(), RenderError> {
    assert!(check_single_oidc_writer(&jobs(false)?).is_ok());
    assert!(check_single_oidc_writer(&jobs(true)?).is_ok());
    let mut none = jobs(false)?;
    none.get_mut("publish").expect("oidc").permissions.id_token = PermissionLevel::None;
    let problem = problem_of(check_single_oidc_writer(&none))?;
    assert!(problem.contains("id_token_writer_count:0"), "{problem}");
    let mut two = jobs(true)?;
    two.get_mut("publish-bootstrap")
        .expect("bootstrap")
        .permissions
        .id_token = PermissionLevel::Write;
    let problem = problem_of(check_single_oidc_writer(&two))?;
    assert!(problem.contains("id_token_writer_count:2"), "{problem}");
    let mut wrong = BTreeMap::new();
    wrong.insert(
        "preparation".to_owned(),
        job(ReleaseRole::Preparation, &[], None)?,
    );
    wrong
        .get_mut("preparation")
        .expect("prep")
        .permissions
        .id_token = PermissionLevel::Write;
    let problem = problem_of(check_single_oidc_writer(&wrong))?;
    assert!(problem.contains("id_token_writer_role"), "{problem}");
    Ok(())
}

#[test]
fn environment_names_are_pinned() -> Result<(), RenderError> {
    assert!(validate_environment(ENV_OIDC).is_ok());
    assert!(validate_environment("team/app.env-1").is_ok());
    for bad in ["", "has space", "has${{ x }}", "has\nline"] {
        assert!(validate_environment(bad).is_err(), "{bad:?} must fail");
    }
    let mut bad = spec(false)?;
    bad.publish_environment = "has space".to_owned();
    let problem = problem_of(bad.validate())?;
    assert!(problem.contains("bad_environment"), "{problem}");
    Ok(())
}

#[test]
fn push_branches_are_exact_never_globs() -> Result<(), RenderError> {
    assert!(triggers().validate(&bootstrap()).is_ok());
    for bad in ["main*", "feature/*", "release?", "[abc]", "has space", ""] {
        let mut case = triggers();
        case.push_branches = vec![bad.to_owned()];
        let problem = problem_of(case.validate(&bootstrap()))?;
        assert!(problem.contains("bad_push_branch"), "{bad}: {problem}");
    }
    let mut case = triggers();
    case.push_branches.clear();
    assert!(problem_of(case.validate(&bootstrap()))?.contains("no_push_branch"));
    case.push_branches = (0..9).map(|n| format!("b{n}")).collect();
    assert!(problem_of(case.validate(&bootstrap()))?.contains("no_push_branch"));
    Ok(())
}

#[test]
fn bootstrap_repository_must_match_spec() -> Result<(), RenderError> {
    let mut bad = spec(false)?;
    bad.bootstrap.repository = "evil/fork".to_owned();
    let problem = problem_of(bad.validate())?;
    assert!(
        problem.contains("bootstrap_repository_mismatch"),
        "{problem}"
    );
    assert!(validate_repository("evil/fork").is_ok());
    for bad_repo in ["no-slash", "a/b/c", "", "acme/widgets "] {
        assert!(validate_repository(bad_repo).is_err(), "{bad_repo:?}");
    }
    Ok(())
}

#[test]
fn publish_gate_binds_repo_plan_source() -> Result<(), RenderError> {
    let gate = publish_gate_condition(REPOSITORY, &bootstrap());
    assert!(gate.contains(REPOSITORY), "{gate}");
    assert!(gate.contains(PLAN_ID), "{gate}");
    assert!(gate.contains(SHA), "{gate}");
    let fork_gate = publish_gate_condition("evil/fork", &bootstrap());
    assert_ne!(gate, fork_gate, "fork repo must change the gate");
    let mut bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("oidc").condition = Some("true".to_owned());
    let problem = problem_of(bad.validate())?;
    assert!(problem.contains("publish_gate_mismatch"), "{problem}");
    Ok(())
}

#[test]
fn full_role_sets_validate() -> Result<(), RenderError> {
    assert!(spec(false)?.validate().is_ok());
    assert!(spec(true)?.validate().is_ok());
    let mut missing = spec(false)?;
    missing.jobs.remove("publish");
    let problem = problem_of(missing.validate())?;
    assert!(problem.contains("release_role_set"), "{problem}");
    let mut extra = spec(true)?;
    extra.jobs.insert(
        "spare-preflight".to_owned(),
        job(ReleaseRole::Preflight, &[], None)?,
    );
    assert!(problem_of(extra.validate())?.contains("release_role_set"));
    Ok(())
}

#[test]
fn publish_and_reconcile_needs_are_forward() -> Result<(), RenderError> {
    let mut bad = spec(false)?;
    bad.jobs.get_mut("publish").expect("oidc").needs.clear();
    let problem = problem_of(bad.validate())?;
    assert!(problem.contains("publish_without_preflight"), "{problem}");
    bad = spec(false)?;
    bad.jobs.get_mut("reconcile").expect("gate").needs.clear();
    let problem = problem_of(bad.validate())?;
    assert!(problem.contains("reconcile_without_publish"), "{problem}");
    bad = spec(false)?;
    bad.jobs
        .get_mut("preflight")
        .expect("preflight")
        .needs
        .push("publish".to_owned());
    let problem = problem_of(bad.validate())?;
    assert!(problem.contains("backward_need"), "{problem}");
    let live = jobs(false)?;
    let ids: BTreeSet<&str> = live.keys().map(String::as_str).collect();
    assert_eq!(ids.len(), 4);
    Ok(())
}

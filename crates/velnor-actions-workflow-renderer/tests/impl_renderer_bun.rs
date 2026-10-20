//! Bun member jobs (hard decision 6): shape, permissions and role gates.

use velnor_actions_contract::{
    JobTimeout, PRIVATE_NPM_READ_EXPR, PRIVATE_NPM_READ_TOKEN_ENV, PermissionLevel, Step, StepKind,
    StepRole,
};
use velnor_actions_workflow_renderer::verification_jobs::{
    BUN_RUNNER, BunMemberPolicy, build_bun_member_job,
};
use velnor_actions_workflow_renderer::{RenderError, shell_step};

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

fn step(name: &str, parallel: bool) -> Result<Step, RenderError> {
    let mut step = shell_step(
        name,
        vec!["true".to_owned()],
        std::collections::BTreeMap::new(),
    )?;
    step.parallel = parallel;
    Ok(step)
}

fn policy(steps: Vec<Step>) -> BunMemberPolicy {
    BunMemberPolicy {
        name: "@chainargos/ui".to_owned(),
        id: "bun-chainargos-ui".to_owned(),
        timeout: JobTimeout::DEFAULT,
        steps,
    }
}

#[test]
fn a_member_job_is_hosted_unprivileged_and_gated_by_the_plan() -> Result<(), RenderError> {
    let job = build_bun_member_job(
        &policy(vec![step("Install", false)?, step("Test", true)?]),
        CHECKOUT,
    )?;
    assert_eq!(job.display_name, "Bun / @chainargos/ui");
    assert_eq!(job.runs_on, BUN_RUNNER);
    assert_eq!(job.needs, ["plan"]);
    let permissions = job.permissions.expect("permissions");
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.packages, PermissionLevel::None);
    assert!(job.condition.expect("gate").contains(",bun-chainargos-ui,"));
    assert_eq!(job.steps[0].name, "Checkout");
    Ok(())
}

#[test]
fn invalid_policies_are_rejected() -> Result<(), RenderError> {
    let reject = |policy: BunMemberPolicy, code: &str| {
        let error = build_bun_member_job(&policy, CHECKOUT).expect_err(code);
        assert!(error.to_string().contains(code), "{code}: {error}");
    };
    reject(policy(Vec::new()), "bun_job_steps_empty");
    let mut wrong_id = policy(vec![step("Test", false)?]);
    wrong_id.id = "ui".to_owned();
    reject(wrong_id, "bun_job_id");
    reject(
        policy(vec![
            step("A", true)?,
            step("Install", false)?,
            step("B", true)?,
        ]),
        "bun_parallel_group_split",
    );
    let mut mint = step("Mint", false)?;
    mint.role = Some(StepRole::PrivateTokenMint);
    reject(policy(vec![mint]), "bun_job_app_token_step");
    let mut app_fetch = step("Fetch", false)?;
    app_fetch.role = Some(StepRole::PrivateFetch);
    reject(policy(vec![app_fetch]), "bun_job_app_token_step");
    Ok(())
}

#[test]
fn only_the_npm_registry_phase_grants_packages_read() -> Result<(), RenderError> {
    let mut registry = step("Install dependencies (registry phase)", false)?;
    registry.kind = StepKind::Shell {
        run: vec!["true".to_owned()],
        env: std::collections::BTreeMap::from([(
            PRIVATE_NPM_READ_TOKEN_ENV.to_owned(),
            PRIVATE_NPM_READ_EXPR.to_owned(),
        )]),
    };
    registry.role = Some(StepRole::PrivateFetch);
    let job = build_bun_member_job(
        &policy(vec![registry.clone(), step("Test", true)?]),
        CHECKOUT,
    )?;
    assert_eq!(
        job.permissions.expect("permissions").packages,
        PermissionLevel::Read
    );
    let error = build_bun_member_job(&policy(vec![registry.clone(), registry]), CHECKOUT)
        .expect_err("two registry phases");
    assert!(
        error.to_string().contains("bun_registry_phase_repeated"),
        "{error}"
    );
    Ok(())
}

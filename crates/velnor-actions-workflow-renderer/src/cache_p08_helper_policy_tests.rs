use velnor_actions_contract::{Job, JobTimeout, Step, StepRole};

use super::*;

const CHECKOUT: &str = "actions/checkout@0000000000000000000000000000000000000000";
const TARGET: &str = "x86_64-unknown-linux-gnu";

fn setup() -> crate::MiseSetup {
    crate::MiseSetup {
        uses: "jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7".to_owned(),
        version: "2026.10.13".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn job_with(steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Cache helper policy".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    }
}

fn mise_shell() -> Step {
    crate::steps::shell_step(
        "Run Cargo",
        vec![
            "mise".to_owned(),
            "exec".to_owned(),
            "rust@1.99.1".to_owned(),
            "--".to_owned(),
            "cargo".to_owned(),
            "check".to_owned(),
        ],
        std::collections::BTreeMap::new(),
    )
    .expect("Mise task step")
}

fn add_cache_policy(
    job_id: &str,
    job: &mut Job,
    require_verified_helper: bool,
) -> Result<(), RenderError> {
    crate::cache_p08::ensure_tools_cache_v2_with_helper_policy(
        job_id,
        job,
        &setup(),
        false,
        crate::cache_p08::ToolsCacheRenderOptions {
            target: TARGET,
            scale_set_image_source_sha: None,
            checkout_uses: CHECKOUT,
            require_verified_helper,
        },
    )
}

#[test]
fn bootstrap_without_helper_keeps_setup_and_task_but_omits_cache_actions() {
    let mut job = job_with(vec![
        crate::steps::checkout_step(CHECKOUT).expect("checkout"),
        mise_shell(),
    ]);

    add_cache_policy("source-bootstrap", &mut job, false).expect("cold bootstrap");

    assert!(job.steps.iter().any(|step| step.name == "Setup Mise"));
    assert!(job.steps.iter().any(|step| step.name == "Run Cargo"));
    assert!(job.steps.iter().all(|step| {
        !matches!(
            step.role,
            Some(
                StepRole::ToolsCacheIdentity
                    | StepRole::ToolsCacheRestore
                    | StepRole::ToolsCacheSave
                    | StepRole::ToolSeed
            )
        )
    }));
}

#[test]
fn required_helper_missing_is_a_generation_error() {
    let mut job = job_with(vec![
        crate::steps::checkout_step(CHECKOUT).expect("checkout"),
        mise_shell(),
    ]);

    let error = add_cache_policy("consumer-job", &mut job, true).expect_err("helper required");
    assert!(format!("{error:?}").contains("tools_cache_helper_missing:consumer-job"));
    assert!(job.steps.iter().any(|step| step.name == "Setup Mise"));
    assert!(job.steps.iter().any(|step| step.name == "Run Cargo"));
    assert!(
        job.steps
            .iter()
            .all(|step| step.role != Some(StepRole::ToolsCacheIdentity))
    );
}

#[test]
fn cache_restore_runs_after_mise_setup_and_before_mise_use() -> Result<(), RenderError> {
    let mut job = job_with(vec![
        crate::steps::checkout_step(CHECKOUT)?,
        super::verified_acquire_step()?,
        mise_shell(),
    ]);

    add_cache_policy("verified-job", &mut job, true)?;

    let setup_index = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::MiseSetup))
        .ok_or_else(|| RenderError::InvalidWorkflow("setup_missing".to_owned()))?;
    let helper_index = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::AcquireVelnor))
        .ok_or_else(|| RenderError::InvalidWorkflow("verified_helper_missing".to_owned()))?;
    let cache_index = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::ToolsCacheIdentity))
        .ok_or_else(|| RenderError::InvalidWorkflow("cache_identity_missing".to_owned()))?;
    let restore_index = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::ToolsCacheRestore))
        .ok_or_else(|| RenderError::InvalidWorkflow("cache_restore_missing".to_owned()))?;
    let task_index = job
        .steps
        .iter()
        .position(|step| step.name == "Run Cargo")
        .ok_or_else(|| RenderError::InvalidWorkflow("task_missing".to_owned()))?;

    assert!(helper_index < cache_index);
    assert!(cache_index < restore_index);
    assert!(setup_index < cache_index);
    assert!(cache_index < restore_index);
    assert!(restore_index < task_index);
    assert_eq!(
        job.steps[cache_index].name,
        crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME
    );
    Ok(())
}

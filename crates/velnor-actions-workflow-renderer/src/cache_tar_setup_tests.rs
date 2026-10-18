use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind, StepRole, workflow::JobTimeout};

use super::{insert_launcher_steps, is_launcher, strip_preseed_bootstrap_cache};

#[test]
fn keeps_verified_helper_order_and_installs_adapters_before_cache_and_internal_steps()
-> Result<(), crate::RenderError> {
    let mut job = job(vec![
        step("Checkout", StepRole::Checkout, shell()),
        acquire_step(),
        step("Restore", StepRole::CargoSourcesRestore, cache_action()),
        step("Plan", StepRole::PlanProducer, internal("plan-v1")),
    ]);
    let mut jobs = BTreeMap::from([("task".to_owned(), job.clone())]);

    insert_launcher_steps(&mut jobs)?;

    job = jobs
        .remove("task")
        .ok_or_else(|| crate::RenderError::InvalidWorkflow("test_task_job_missing".to_owned()))?;
    assert_eq!(
        job.steps
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Checkout",
            "Acquire Velnor",
            "Install cache tar adapters",
            "Restore",
            "Plan",
        ]
    );
    assert_eq!(job.steps.iter().filter(|step| is_launcher(step)).count(), 1);
    Ok(())
}

#[test]
fn helper_before_checkout_is_rejected_without_reordering() -> Result<(), crate::RenderError> {
    let mut jobs = BTreeMap::from([(
        "task".to_owned(),
        job(vec![
            acquire_step(),
            step("Checkout", StepRole::Checkout, shell()),
            step("Restore", StepRole::CargoSourcesRestore, cache_action()),
        ]),
    )]);

    let Err(error) = insert_launcher_steps(&mut jobs) else {
        return Err(crate::RenderError::InvalidWorkflow(
            "test_expected_helper_order_error".to_owned(),
        ));
    };

    assert!(
        error
            .to_string()
            .contains("cache_helper_before_checkout:task")
    );
    Ok(())
}

#[test]
fn launcher_insertion_is_idempotent_and_leaves_jobs_without_archive_cache_unchanged()
-> Result<(), crate::RenderError> {
    let task = job(vec![
        step("Checkout", StepRole::Checkout, shell()),
        acquire_step(),
        step("Restore", StepRole::CargoSourcesRestore, cache_action()),
    ]);
    let plain = job(vec![step(
        "Run",
        StepRole::PlanProducer,
        internal("plan-v1"),
    )]);
    let mut jobs = BTreeMap::from([
        ("plain".to_owned(), plain.clone()),
        ("task".to_owned(), task),
    ]);

    insert_launcher_steps(&mut jobs)?;
    insert_launcher_steps(&mut jobs)?;

    assert_eq!(jobs["plain"].steps, plain.steps);
    assert_eq!(
        jobs["task"]
            .steps
            .iter()
            .filter(|step| is_launcher(step))
            .count(),
        1
    );
    Ok(())
}

#[test]
fn cache_action_without_a_helper_stage_fails_closed() -> Result<(), crate::RenderError> {
    let mut jobs = BTreeMap::from([(
        "task".to_owned(),
        job(vec![step(
            "Restore",
            StepRole::CargoSourcesRestore,
            cache_action(),
        )]),
    )]);

    let Err(error) = insert_launcher_steps(&mut jobs) else {
        return Err(crate::RenderError::InvalidWorkflow(
            "test_expected_missing_helper_error".to_owned(),
        ));
    };

    assert!(
        error
            .to_string()
            .contains("cache_action_helper_stage_count:task:0")
    );
    Ok(())
}

#[test]
fn cache_action_before_preseed_helper_fails_without_reordering_build_work()
-> Result<(), crate::RenderError> {
    let mut jobs = BTreeMap::from([(
        "task".to_owned(),
        job(vec![
            step("Checkout", StepRole::Checkout, shell()),
            step("Restore", StepRole::CargoSourcesRestore, cache_action()),
            step("Build helper", StepRole::PreseedBuild, shell()),
            preseed_stage_step(),
        ]),
    )]);

    let Err(error) = insert_launcher_steps(&mut jobs) else {
        return Err(crate::RenderError::InvalidWorkflow(
            "test_expected_helper_order_error".to_owned(),
        ));
    };

    assert!(
        error
            .to_string()
            .contains("cache_action_before_helper:task")
    );
    assert_eq!(jobs["task"].steps[2].name, "Build helper");
    Ok(())
}

#[test]
fn preseed_stage_can_provide_the_verified_helper() -> Result<(), crate::RenderError> {
    let mut jobs = BTreeMap::from([(
        "task".to_owned(),
        job(vec![
            step("Checkout", StepRole::Checkout, shell()),
            preseed_stage_step(),
            step(
                "Restore",
                StepRole::TofuProvidersRestore,
                action("./.github/actions/tofu-provider-admission"),
            ),
        ]),
    )]);

    insert_launcher_steps(&mut jobs)?;

    let steps = &jobs
        .get("task")
        .ok_or_else(|| crate::RenderError::InvalidWorkflow("test_task_job_missing".to_owned()))?
        .steps;
    assert_eq!(steps[2].name, "Install cache tar adapters");
    assert_eq!(steps[3].role, Some(StepRole::TofuProvidersRestore));
    let launcher_inputs = match &steps[2].kind {
        StepKind::Action { with, env, .. } if env.is_empty() => with,
        _ => {
            return Err(crate::RenderError::InvalidWorkflow(
                "test_launcher_action_shape".to_owned(),
            ));
        }
    };
    assert_eq!(
        launcher_inputs
            .get(crate::steps::CACHE_TAR_LAUNCHER_SHA256_INPUT)
            .map(String::as_str),
        Some(crate::steps::PRESEED_HELPER_SHA256_OUTPUT_EXPR)
    );
    Ok(())
}

#[test]
fn preseed_bootstrap_drops_only_cache_steps_before_local_helper_exists() {
    let mut job = job(vec![
        step("Checkout", StepRole::Checkout, shell()),
        step(
            "Restore sources",
            StepRole::CargoSourcesRestore,
            cache_action(),
        ),
        step(
            "Tool identity",
            StepRole::ToolsCacheIdentity,
            action("local"),
        ),
        step(
            "Cache stage",
            StepRole::NormalToolsCacheStage,
            internal("stage-v1"),
        ),
        step(
            "Cache import",
            StepRole::NormalToolsCacheImport,
            internal("import-v1"),
        ),
        step(
            "Cache export",
            StepRole::NormalToolsCacheExport,
            internal("export-v1"),
        ),
        step("Seed", StepRole::ToolSeed, action("local")),
        step("Build helper", StepRole::PreseedBuild, shell()),
        preseed_stage_step(),
        step("Plan", StepRole::PlanProducer, internal("plan-v1")),
    ]);

    strip_preseed_bootstrap_cache(&mut job);

    assert_eq!(
        job.steps
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        ["Checkout", "Build helper", "Stage helper", "Plan"]
    );
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Cache job".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        steps,
    }
}

fn step(name: &str, role: StepRole, kind: StepKind) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: Some(role),
        condition: None,
        kind,
    }
}

fn acquire_step() -> Step {
    step(
        "Acquire Velnor",
        StepRole::AcquireVelnor,
        StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::from([(crate::steps::ASSET_SHA_ENV.to_owned(), "a".repeat(64))]),
        },
    )
}

fn preseed_stage_step() -> Step {
    let mut step = step("Stage helper", StepRole::PreseedStage, shell());
    step.id = Some(velnor_actions_contract::StepId::PreseedStage);
    step
}

fn shell() -> StepKind {
    StepKind::Shell {
        run: vec!["true".to_owned()],
        env: BTreeMap::new(),
    }
}

fn internal(operation: &str) -> StepKind {
    StepKind::Internal {
        operation: operation.to_owned(),
        env: BTreeMap::new(),
    }
}

fn cache_action() -> StepKind {
    action("actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9")
}

fn action(uses: &str) -> StepKind {
    StepKind::Action {
        uses: uses.to_owned(),
        with: BTreeMap::new(),
        env: BTreeMap::new(),
    }
}

//! Workflow job constructors: plan, task, lint, and final gate.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{Job, Step, StepKind};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_workflow_renderer::render::{
    FINAL_CONDITION, FINAL_DISPLAY_NAME, MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV,
    MATRIX_OUTPUT_ENV, PLAN_JOB_ID, TASK_JOB_ID,
};
use velnor_actions_workflow_renderer::steps::{merge_step, plan_step};

use crate::OrchestratorError;
use crate::workflow::CHECKOUT_USES;

/// Always-on workflow-lint job ID, emitted for both policies.
pub(crate) const LINT_JOB_ID: &str = "velnor-workflow-lint";

/// Display name of the always-on workflow-lint job.
pub(crate) const LINT_DISPLAY_NAME: &str = "Velnor Workflow Lint";

/// Matrix entry task ID consumed by the fixed task template.
const TASK_ID_ENV: &str = "VELNOR_TASK_ID";

/// Matrix entry command consumed by the fixed task template.
const TASK_RUN_ENV: &str = "VELNOR_TASK_RUN";

/// Producer output carrying the matrix JSON for `fromJSON`.
const MATRIX_OUTPUT_NAME: &str = "matrix";

/// Planner job: checkout, optional Acquire Velnor, plus the plan step.
pub(crate) fn plan_job(label: &str, acquire: Option<Step>) -> Job {
    let mut steps = vec![checkout_action()];
    steps.extend(acquire);
    steps.push(plan_step());
    Job {
        display_name: "Velnor Plan".to_owned(),
        runs_on: label.to_owned(),
        needs: Vec::new(),
        condition: None,
        steps,
    }
}

/// Matrix consumer job: checkout plus the fixed matrix-entry template.
///
/// No stack logic: every matrix leg runs the same template, which logs
/// `matrix.task_id` and executes `matrix.run`. The marker trio directs
/// the renderer to the producer; it never renders.
pub(crate) fn task_job(label: &str, max_parallel_jobs: u32) -> Job {
    Job {
        display_name: "Velnor Task".to_owned(),
        runs_on: label.to_owned(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        steps: vec![checkout_action(), matrix_task_step(max_parallel_jobs)],
    }
}

/// Always-on lint job: checkout plus pinned actionlint over the tree.
pub(crate) fn lint_job(label: &str, catalog: &ToolCatalog) -> Result<Job, OrchestratorError> {
    let program = OsString::from("actionlint");
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Actionlint, PinnedTool::Shellcheck],
        &program,
        vec![OsString::from("-color")],
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let argv = strings_of(exec.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    Ok(Job {
        display_name: LINT_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        needs: Vec::new(),
        condition: None,
        steps: vec![
            checkout_action(),
            Step {
                name: "Run actionlint".to_owned(),
                kind: StepKind::Shell {
                    run: argv,
                    env: BTreeMap::new(),
                },
            },
        ],
    })
}

/// Final gate with the exact required-check name and `always()` condition.
///
/// Needs decision (workflow-contract §4 "depends on the base and enabled
/// policy jobs" + "`Velnor / Required` depends on the plan, every selected
/// task result ... and the candidate report when candidate mode is
/// enabled"): plan + task + lint always, plus alint/policy/candidate when
/// those policy jobs exist. IR validation requires `needs` to name IR jobs
/// only, so this builds the IR subset and the renderer appends the merged
/// support IDs post-merge (see `support.rs`); the release job never gates.
pub(crate) fn final_job(label: &str, with_task: bool, acquire: Option<Step>) -> Job {
    let mut needs = vec![PLAN_JOB_ID.to_owned()];
    if with_task {
        needs.push(TASK_JOB_ID.to_owned());
    }
    needs.push(LINT_JOB_ID.to_owned());
    let mut steps = Vec::new();
    steps.extend(acquire);
    steps.push(merge_step());
    Job {
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        steps,
    }
}

/// Pinned checkout action without persisted credentials.
fn checkout_action() -> Step {
    let mut with = BTreeMap::new();
    with.insert("persist-credentials".to_owned(), "false".to_owned());
    Step {
        name: "Checkout".to_owned(),
        kind: StepKind::Action {
            uses: CHECKOUT_USES.to_owned(),
            with,
        },
    }
}

/// Fixed matrix-entry template: log the task ID, then run its command.
///
/// A missing `matrix.run` fails the leg via `${VAR:?...}` instead of a
/// silent no-op. Matrix context arrives via env only, keeping `run:`
/// free of `${{ }}` for shellcheck and template-injection scans.
fn matrix_task_step(max_parallel_jobs: u32) -> Step {
    let script = format!(
        "echo \"${TASK_ID_ENV}\" && : \"${{{TASK_RUN_ENV}:?matrix.run_missing}}\" && sh -c \"${TASK_RUN_ENV}\""
    );
    let env = BTreeMap::from([
        (TASK_ID_ENV.to_owned(), "${{ matrix.task_id }}".to_owned()),
        (TASK_RUN_ENV.to_owned(), "${{ matrix.run }}".to_owned()),
        (MATRIX_NEEDS_JOB_ENV.to_owned(), PLAN_JOB_ID.to_owned()),
        (MATRIX_OUTPUT_ENV.to_owned(), MATRIX_OUTPUT_NAME.to_owned()),
        (
            MATRIX_MAX_PARALLEL_ENV.to_owned(),
            max_parallel_jobs.to_string(),
        ),
    ]);
    Step {
        name: "Run task".to_owned(),
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), script],
            env,
        },
    }
}

/// Convert fixed argv to UTF-8 strings.
fn strings_of(argv: Vec<OsString>) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(argv.len());
    for arg in argv {
        match arg.into_string() {
            Ok(text) => out.push(text),
            Err(_) => return Err("non_utf8_argv".to_owned()),
        }
    }
    Ok(out)
}

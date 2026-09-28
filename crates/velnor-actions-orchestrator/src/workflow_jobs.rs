//! Workflow job constructors: plan, task, lint, and final gate.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{Job, Step, StepKind};
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog};
use velnor_actions_rust::{TaskGroup, TaskKind};
use velnor_actions_workflow_renderer::render::{
    FINAL_CONDITION, FINAL_DISPLAY_NAME, PLAN_JOB_ID, TASK_JOB_ID,
};
use velnor_actions_workflow_renderer::steps::{merge_step, plan_step};

use crate::OrchestratorError;
use crate::vectors::task_argv;
use crate::workflow::CHECKOUT_USES;

/// Always-on workflow-lint job ID, emitted for both policies.
pub(crate) const LINT_JOB_ID: &str = "velnor-workflow-lint";

/// Display name of the always-on workflow-lint job.
pub(crate) const LINT_DISPLAY_NAME: &str = "Velnor Workflow Lint";

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

/// Matrix consumer job: checkout plus one fixed vector per runnable group.
pub(crate) fn task_job(
    label: &str,
    groups: &[&TaskGroup],
    catalog: &ToolCatalog,
) -> Result<Job, OrchestratorError> {
    let mut steps = Vec::with_capacity(groups.len() + 1);
    steps.push(checkout_action());
    for group in groups {
        steps.push(task_step(group, catalog)?);
    }
    Ok(Job {
        display_name: "Velnor Task".to_owned(),
        runs_on: label.to_owned(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        steps,
    })
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
pub(crate) fn final_job(label: &str, with_task: bool) -> Job {
    let mut needs = vec![PLAN_JOB_ID.to_owned()];
    if with_task {
        needs.push(TASK_JOB_ID.to_owned());
    }
    needs.push(LINT_JOB_ID.to_owned());
    Job {
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        steps: vec![merge_step()],
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

/// One fixed-vector step for a runnable task group.
fn task_step(group: &TaskGroup, catalog: &ToolCatalog) -> Result<Step, OrchestratorError> {
    let argv = task_argv(group, catalog)?;
    Ok(Step {
        name: task_step_name(group),
        kind: StepKind::Shell {
            run: argv,
            env: BTreeMap::new(),
        },
    })
}

/// Display name derived from group kind, package, and configuration.
fn task_step_name(group: &TaskGroup) -> String {
    let kind = match group.kind {
        TaskKind::Fmt => "Format",
        TaskKind::Clippy => "Clippy",
        TaskKind::Test => "Test",
        TaskKind::Nextest => "Nextest",
        TaskKind::Doctest => "Doctests",
        TaskKind::Doc => "Doc",
        TaskKind::Build => "Build",
    };
    let what = if group.package_name.is_empty() {
        "workspace".to_owned()
    } else {
        group.package_name.clone()
    };
    format!("{kind} {what} ({})", group.configuration)
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

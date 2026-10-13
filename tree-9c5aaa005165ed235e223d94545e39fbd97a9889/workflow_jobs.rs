//! Workflow job constructors: plan, task, lint, and final gate.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{Job, Step, StepKind};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PinnedToolExec, PreparePinnedTools, ToolCatalog,
    ToolHomes,
};
use velnor_actions_workflow_renderer::render::{
    FINAL_CONDITION, FINAL_DISPLAY_NAME, MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV,
    MATRIX_OUTPUT_ENV, PLAN_JOB_ID, TASK_JOB_ID,
};
use velnor_actions_workflow_renderer::steps::{
    MERGE_OPERATION, PLAN_OPERATION, merge_step, plan_step, write_request_step,
};

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

/// Planner job: checkout, pinned-tool install, optional Acquire, request, plan.
///
/// `Prepare pinned tools` installs the exact catalog tools the later steps
/// consume through fail-closed `mise exec`: Rust plus the detected MBX
/// driver for the format/build steps, and the actionlint/shellcheck/zizmor
/// validators the public `generate` runs inside `Check generated files`.
/// Without it the freshness step fails with `mise ... couldn't exec
/// process` because implicit installation is disabled there.
///
/// No `Verify toolchain` step: task-execution-contract §2 scopes it to
/// task jobs; the plan sequence is workflow-contract §3 steps 1-8.
///
/// The write-request step materializes the event request file the plan
/// step's private gate requires; without it the helper falls through to
/// CLI usage and the job (plus its `Publish plan` upload) fails.
/// # Errors
///
/// Returns a contract error when a typed step request is rejected.
pub(crate) fn plan_job(
    label: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    use_mbx: bool,
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![checkout_action()];
    steps.push(prepare_pinned_tools_step(catalog, plan_tools(use_mbx))?);
    steps.extend(acquire);
    steps.push(request_step(PLAN_OPERATION)?);
    steps.push(plan_step());
    Ok(Job {
        display_name: "Velnor Plan".to_owned(),
        runs_on: label.to_owned(),
        needs: Vec::new(),
        condition: None,
        steps,
    })
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
/// The write-request step assembles the merge request from the downloaded
/// plan and matrix-report artifacts the merge step consumes.
/// # Errors
///
/// Returns a contract error when the typed request step is rejected.
pub(crate) fn final_job(
    label: &str,
    with_task: bool,
    acquire: Option<Step>,
) -> Result<Job, OrchestratorError> {
    let mut needs = vec![PLAN_JOB_ID.to_owned()];
    if with_task {
        needs.push(TASK_JOB_ID.to_owned());
    }
    needs.push(LINT_JOB_ID.to_owned());
    let mut steps = Vec::new();
    steps.extend(acquire);
    steps.push(request_step(MERGE_OPERATION)?);
    steps.push(merge_step());
    Ok(Job {
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        steps,
    })
}

/// Plan-job install set: driver tools plus the `generate` validators.
///
/// Validators join the driver set because `Check generated files` runs the
/// public `generate`, whose staged validation fail-closed-execs pinned
/// actionlint, shellcheck, and zizmor; installing only the driver
/// toolchain leaves that step red. Order follows `PinnedTool::ALL`.
fn plan_tools(use_mbx: bool) -> Vec<PinnedTool> {
    let mut tools = vec![PinnedTool::Rust];
    if use_mbx {
        tools.push(PinnedTool::MrBoxington);
    }
    tools.extend([
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ]);
    tools
}

/// Typed `Prepare pinned tools` step for one exact tool set.
///
/// Homes use the runner-temp expression form: shell `$VAR` never expands
/// in the `env:` position that carries these paths.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
fn prepare_pinned_tools_step(
    catalog: &ToolCatalog,
    tools: Vec<PinnedTool>,
) -> Result<Step, OrchestratorError> {
    let homes = ToolHomes::new(
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/cargo",
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let prepare =
        PreparePinnedTools::new(tools, homes).map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let run = strings_of(prepare.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = strings_of_env(&prepare.env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    Ok(Step {
        name: PREPARE_PINNED_TOOLS_STEP.to_owned(),
        kind: StepKind::Shell { run, env },
    })
}

/// Typed write-request step for one internal target, mapped to contract errors.
fn request_step(target: &str) -> Result<Step, OrchestratorError> {
    write_request_step(target).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
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

/// Convert fixed env pairs to UTF-8 strings.
fn strings_of_env(env: &[(OsString, OsString)]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (key, value) in env {
        let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else {
            return Err("non_utf8_env".to_owned());
        };
        out.insert(key.to_owned(), value.to_owned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Internal operation of one step, if any.
    fn operation_of(step: &Step) -> Option<&str> {
        match &step.kind {
            StepKind::Internal { operation } => Some(operation),
            StepKind::Action { .. } | StepKind::Shell { .. } => None,
        }
    }

    #[test]
    fn plan_job_writes_request_before_plan() {
        let catalog = ToolCatalog::pinned();
        for acquire in [None, Some(checkout_action())] {
            let job = plan_job("ubuntu-26.04", acquire, &catalog, false).expect("plan job");
            let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
            let write_at = names.iter().position(|name| *name == "Write request");
            let plan_at = names.iter().position(|name| *name == "Plan");
            assert!(
                write_at.is_some_and(|write| Some(write) < plan_at),
                "request must precede plan: {names:?}"
            );
            let write = &job.steps[write_at.expect("write request step")];
            assert_eq!(
                operation_of(write),
                Some("write-request-v1:plan-v1"),
                "request must target plan"
            );
            assert_eq!(
                operation_of(&job.steps[plan_at.expect("plan step")]),
                Some(PLAN_OPERATION)
            );
        }
    }

    #[test]
    fn plan_job_prepares_pinned_tools_before_generate_consumers() {
        let catalog = ToolCatalog::pinned();
        for use_mbx in [false, true] {
            let job = plan_job("ubuntu-26.04", None, &catalog, use_mbx).expect("plan job");
            let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
            let prepare_at = names
                .iter()
                .position(|name| *name == PREPARE_PINNED_TOOLS_STEP);
            assert_eq!(
                prepare_at,
                Some(1),
                "prepare sits after checkout: {names:?}"
            );
            let write_at = names.iter().position(|name| *name == "Write request");
            let plan_at = names.iter().position(|name| *name == "Plan");
            assert!(
                prepare_at
                    .is_some_and(|prepare| Some(prepare) < write_at && Some(prepare) < plan_at),
                "prepare must precede request and plan: {names:?}"
            );
            let StepKind::Shell { run, env } = &job.steps[prepare_at.expect("prepare step")].kind
            else {
                panic!("prepare must be a shell step: {names:?}");
            };
            assert_eq!(run[0], "mise");
            let install_at = run.iter().position(|arg| arg == "install");
            let mut specs = vec![
                catalog.tool_spec(PinnedTool::Rust),
                catalog.tool_spec(PinnedTool::Actionlint),
                catalog.tool_spec(PinnedTool::Shellcheck),
                catalog.tool_spec(PinnedTool::Zizmor),
            ];
            if use_mbx {
                specs.insert(1, catalog.tool_spec(PinnedTool::MrBoxington));
            }
            assert_eq!(
                install_at.map(|at| &run[at + 1..]),
                Some(specs.as_slice()),
                "install specs: {run:?}"
            );
            for key in [
                "MISE_RUSTUP_HOME",
                "MISE_CARGO_HOME",
                "RUSTUP_TOOLCHAIN",
                "MISE_LOCKFILE",
            ] {
                assert!(env.contains_key(key), "env misses {key}: {env:?}");
            }
        }
    }

    #[test]
    fn final_job_writes_request_before_merge() {
        for acquire in [None, Some(checkout_action())] {
            let job = final_job("ubuntu-26.04", true, acquire).expect("final job");
            let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
            let write_at = names.iter().position(|name| *name == "Write request");
            let merge_at = names.iter().position(|name| *name == "Merge reports");
            assert!(
                write_at.is_some_and(|write| Some(write) < merge_at),
                "request must precede merge: {names:?}"
            );
            let write = &job.steps[write_at.expect("write request step")];
            assert_eq!(
                operation_of(write),
                Some("write-request-v1:merge-v1"),
                "request must target merge"
            );
            assert_eq!(
                operation_of(&job.steps[merge_at.expect("merge step")]),
                Some(MERGE_OPERATION)
            );
        }
    }
}

//! Workflow job constructors: plan, lint, and final gate.
//!
//! Crate jobs live in [`crate::crate_jobs`]: one ordered IR job per
//! crate, each needing the plan job; the final gate below needs them all.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{Job, JobTimeout, Step};
use velnor_actions_mise::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, PinnedToolExec, PreparePinnedTools, ToolCatalog,
    ToolHomes,
};
use velnor_actions_workflow_renderer::render::{FINAL_CONDITION, FINAL_DISPLAY_NAME, PLAN_JOB_ID};
use velnor_actions_workflow_renderer::steps::{
    MERGE_OPERATION, PLAN_OPERATION, merge_step, plan_step, write_request_step,
};

use crate::OrchestratorError;
use crate::source_prep::fetch_steps_for_plan;
use crate::utf8::{strings_of, strings_of_env};

/// Always-on workflow-lint job ID, emitted for both policies.
pub(crate) const LINT_JOB_ID: &str = "actionlint";

/// Display name of the always-on workflow-lint job.
pub(crate) const LINT_DISPLAY_NAME: &str = "Actionlint";

/// Planner job: checkout, pinned-tool install, optional Acquire, request, plan.
///
/// `Prepare pinned tools` installs the exact catalog tools the later steps consume
/// through fail-closed `mise exec`, per role: Rust plus the detected MBX driver
/// for the format/build steps, Nextest when any leg selects it, Opentofu when any
/// tofu work exists, and the validators the public `generate` runs inside `Check
/// generated files`. Without it the freshness step fails with `mise ...
/// couldn't exec process` because implicit installation is disabled there.
/// Pure-tofu plans install opentofu plus the validators with no Rust setup
/// (no components, fetch, or owned-homes triple); mixed plans the union.
///
/// No `Verify toolchain` step: task-execution-contract §2 scopes it to
/// task jobs; the plan sequence is workflow-contract §3 steps 1-8.
///
/// The write-request step materializes the event request file the plan
/// step's private gate requires; without it the helper falls through to
/// CLI usage and the job (plus its `Publish plan` upload) fails.
/// `Fetch Cargo sources` runs `cargo fetch --locked` per lockful
/// workspace ahead of every locked/offline consumer (Gate 1).
/// # Errors
///
/// Returns a contract error when a typed step request is rejected.
pub(crate) fn plan_job(
    label: &str,
    acquire: Option<Step>,
    catalog: &ToolCatalog,
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
    fetch_roots: &[String],
) -> Result<Job, OrchestratorError> {
    let mut steps = vec![checkout_history_action()?];
    let prepare = prepare_pinned_tools_step(
        catalog,
        plan_tools(use_rust, use_mbx, use_nextest, use_opentofu),
        use_rust,
    )?;
    steps.push(prepare);
    if use_rust {
        steps.push(crate::workflow::prepare_rust_components_step(catalog)?);
    }
    let cached =
        crate::workflow_jobs_cache::cache_steps_for_plan(label, catalog, use_mbx, fetch_roots)?;
    steps.extend(cached.restore);
    steps.extend(fetch_steps_for_plan(catalog, fetch_roots)?);
    steps.extend(cached.save);
    steps.extend(acquire);
    steps.push(request_step(PLAN_OPERATION)?);
    steps.push(plan_step());
    Ok(Job {
        display_name: "Plan".to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
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
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            checkout_action()?,
            velnor_actions_workflow_renderer::ambient_shell_step(
                "Run actionlint",
                argv,
                BTreeMap::new(),
            )
            .map_err(|err| OrchestratorError::Contract {
                problem: err.to_string(),
            })?,
        ],
    })
}

/// Final gate with the exact required-check name and `always()` condition.
///
/// Needs decision (workflow-contract §4 "depends on the base and enabled
/// policy jobs" + "`Required` depends on the plan, every crate
/// job ... and the candidate report when candidate mode is enabled"):
/// plan + every crate job + lint always, plus alint/policy/candidate when
/// those policy jobs exist. IR validation requires `needs` to name IR jobs
/// only, so this builds the IR subset and the renderer appends the merged
/// support IDs post-merge (see `support.rs`); the release job never gates.
/// `Prepare pinned tools` installs only `gh`: the fetch step downloads
/// each expected matrix artifact by exact name through it. The
/// write-request step assembles the merge request from the downloaded
/// plan and matrix-report artifacts the merge step consumes.
/// # Errors
///
/// Returns a contract error when a typed step request is rejected.
pub(crate) fn final_job(
    label: &str,
    crate_job_ids: &[String],
    acquire: Option<Step>,
    catalog: &ToolCatalog,
) -> Result<Job, OrchestratorError> {
    let mut needs = vec![PLAN_JOB_ID.to_owned()];
    needs.extend(crate_job_ids.iter().cloned());
    needs.push(LINT_JOB_ID.to_owned());
    let mut steps = Vec::new();
    steps.extend(acquire);
    steps.push(prepare_pinned_tools_step(
        catalog,
        vec![PinnedTool::Gh],
        true,
    )?);
    steps.push(request_step(MERGE_OPERATION)?);
    steps.push(merge_step());
    Ok(Job {
        display_name: FINAL_DISPLAY_NAME.to_owned(),
        runs_on: label.to_owned(),
        timeout_minutes: JobTimeout::REQUIRED,
        needs,
        condition: Some(FINAL_CONDITION.to_owned()),
        permissions: None,
        environment: None,
        steps,
    })
}

/// Plan-job install set per role: drivers, the `generate` validators, Nextest when used.
///
/// Validators join the driver set because `Check generated files` runs the public
/// `generate`, whose staged validation fail-closed-execs pinned actionlint, shellcheck,
/// and zizmor; installing only the driver toolchain leaves that step red. Order
/// follows `PinnedTool::ALL`. Pure-tofu plans carry opentofu plus the validators
/// with no Rust; mixed plans carry the union.
fn plan_tools(
    use_rust: bool,
    use_mbx: bool,
    use_nextest: bool,
    use_opentofu: bool,
) -> Vec<PinnedTool> {
    let mut tools = Vec::new();
    tools.extend(use_rust.then_some(PinnedTool::Rust));
    tools.extend(use_mbx.then_some(PinnedTool::MrBoxington));
    tools.extend([
        PinnedTool::Actionlint,
        PinnedTool::Shellcheck,
        PinnedTool::Zizmor,
    ]);
    tools.extend(use_nextest.then_some(PinnedTool::Nextest));
    tools.extend(use_opentofu.then_some(PinnedTool::Opentofu));
    tools
}

/// Typed `Prepare pinned tools` step for one exact tool set.
///
/// Homes use the runner-temp expression form: shell `$VAR` never expands
/// in the `env:` position that carries these paths. Pure-tofu roles
/// (`use_rust` false) carry no owned-homes triple; every other role
/// keeps it.
/// # Errors
///
/// Returns a contract error when the Mise adapter rejects the request.
fn prepare_pinned_tools_step(
    catalog: &ToolCatalog,
    tools: Vec<PinnedTool>,
    use_rust: bool,
) -> Result<Step, OrchestratorError> {
    let homes = ToolHomes::runner_temp();
    let prepare =
        PreparePinnedTools::new(tools, homes).map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let run = strings_of(prepare.argv(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })?;
    let env = if use_rust {
        strings_of_env(&prepare.env(catalog))
    } else {
        strings_of_env(&prepare.env_without_homes())
    }
    .map_err(|problem| OrchestratorError::Contract { problem })?;
    velnor_actions_workflow_renderer::ambient_shell_step(PREPARE_PINNED_TOOLS_STEP, run, env)
        .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })
}

/// Typed write-request step for one internal target, mapped to contract errors.
fn request_step(target: &str) -> Result<Step, OrchestratorError> {
    write_request_step(target).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

/// Pinned checkout action without persisted credentials.
fn checkout_action() -> Result<Step, OrchestratorError> {
    crate::workflow::wire_w1::checkout_step()
}

/// Pinned checkout with full history for the plan job's git archaeology.
fn checkout_history_action() -> Result<Step, OrchestratorError> {
    crate::workflow::wire_w1::checkout_step_full()
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::StepKind;

    /// Internal operation of one step, if any.
    fn operation_of(step: &Step) -> Option<&str> {
        match &step.kind {
            StepKind::Internal { operation } => Some(operation),
            StepKind::Action { .. } | StepKind::Shell { .. } => None,
        }
    }

    /// Assert Write request precedes `target` with the expected operations.
    fn assert_request_before(job: &Job, target: &str, request: &str, operation: &str) {
        let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
        let write_at = names.iter().position(|name| *name == "Write request");
        let target_at = names.iter().position(|name| *name == target);
        assert!(
            write_at.is_some_and(|write| Some(write) < target_at),
            "request must precede {target}: {names:?}"
        );
        assert_eq!(
            operation_of(&job.steps[write_at.expect("write request step")]),
            Some(request),
            "request must target {target}"
        );
        assert_eq!(
            operation_of(&job.steps[target_at.expect("target step")]),
            Some(operation)
        );
    }

    #[test]
    fn plan_job_writes_request_before_plan() {
        let catalog = ToolCatalog::pinned();
        for acquire in [None, Some(checkout_action().expect("checkout step"))] {
            let job = plan_job("ubuntu-26.04", acquire, &catalog, true, false, false, false, &[])
                .expect("plan job");
            assert_request_before(&job, "Plan", "write-request-v1:plan-v1", PLAN_OPERATION);
        }
    }

    #[test]
    fn plan_job_checks_out_full_history_for_archaeology() {
        let catalog = ToolCatalog::pinned();
        let job = plan_job("ubuntu-26.04", None, &catalog, true, false, false, false, &[])
            .expect("plan job");
        let StepKind::Action { with, .. } = &job.steps[0].kind else {
            panic!("plan must start with checkout");
        };
        assert_eq!(
            with.get("fetch-depth").map(String::as_str),
            Some("0"),
            "plan needs history for HEAD^2 + base diff: {with:?}"
        );
        let lint = lint_job("ubuntu-26.04", &catalog).expect("lint job");
        let StepKind::Action { with, .. } = &lint.steps[0].kind else {
            panic!("lint must start with checkout");
        };
        assert!(
            !with.contains_key("fetch-depth"),
            "lint stays shallow: {with:?}"
        );
    }

    #[test]
    fn plan_tools_follow_role_in_all_order() {
        assert_eq!(
            plan_tools(true, false, false, false),
            vec![
                PinnedTool::Rust,
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor,
            ]
        );
        assert_eq!(
            plan_tools(false, false, false, true),
            vec![
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor,
                PinnedTool::Opentofu,
            ],
            "pure-tofu plans carry opentofu plus the validators, no Rust"
        );
        assert_eq!(
            plan_tools(true, true, true, true),
            vec![
                PinnedTool::Rust,
                PinnedTool::MrBoxington,
                PinnedTool::Actionlint,
                PinnedTool::Shellcheck,
                PinnedTool::Zizmor,
                PinnedTool::Nextest,
                PinnedTool::Opentofu,
            ],
            "mixed plans carry the union"
        );
        for tools in [
            plan_tools(true, false, false, false),
            plan_tools(false, false, false, true),
            plan_tools(true, true, true, true),
        ] {
            let order: Vec<usize> = tools
                .iter()
                .map(|tool| {
                    PinnedTool::ALL
                        .iter()
                        .position(|known| known == tool)
                        .expect("plan installs catalog tools only")
                })
                .collect();
            let mut sorted = order.clone();
            sorted.sort();
            assert_eq!(order, sorted, "plan order follows ALL: {tools:?}");
        }
    }

    #[test]
    fn plan_job_prepares_pinned_tools_before_generate_consumers() {
        let catalog = ToolCatalog::pinned();
        for (use_mbx, use_nextest) in [(false, false), (false, true), (true, true)] {
            let job = plan_job(
                "ubuntu-26.04",
                None,
                &catalog,
                true,
                use_mbx,
                use_nextest,
                false,
                &[],
            )
            .expect("plan job");
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
            if use_nextest {
                specs.push(catalog.tool_spec(PinnedTool::Nextest));
            }
            assert_eq!(
                install_at.map(|at| &run[at + 1..]),
                Some(specs.as_slice()),
                "install specs: {run:?}"
            );
            let keys = [
                "MISE_NO_CONFIG",
                "MISE_RUSTUP_HOME",
                "MISE_CARGO_HOME",
                "RUSTUP_TOOLCHAIN",
            ];
            for key in keys {
                assert!(env.contains_key(key), "env misses {key}: {env:?}");
            }
            assert_eq!(
                env.get("MISE_LOCKFILE").map(String::as_str),
                Some("0"),
                "prepare pins the lockfile off so installs never rewrite it: {env:?}"
            );
        }
    }

    #[test]
    fn pure_tofu_plan_drops_all_rust_setup() {
        use velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP;

        use crate::source_prep::FETCH_SOURCES_STEP;
        let catalog = ToolCatalog::pinned();
        let job = plan_job("ubuntu-26.04", None, &catalog, false, false, false, true, &[])
            .expect("plan job");
        let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
        assert!(
            !names.contains(&PREPARE_RUST_COMPONENTS_STEP),
            "no components step: {names:?}"
        );
        assert!(
            !names.iter().any(|name| name.starts_with(FETCH_SOURCES_STEP)),
            "no cargo fetch: {names:?}"
        );
        let prepare_at = names
            .iter()
            .position(|name| *name == PREPARE_PINNED_TOOLS_STEP)
            .expect("prepare step");
        let StepKind::Shell { run, env } = &job.steps[prepare_at].kind else {
            panic!("prepare must be a shell step: {names:?}");
        };
        let install_at = run
            .iter()
            .position(|arg| arg == "install")
            .expect("install argv");
        let specs = &run[install_at + 1..];
        assert_eq!(
            specs,
            [
                catalog.tool_spec(PinnedTool::Actionlint),
                catalog.tool_spec(PinnedTool::Shellcheck),
                catalog.tool_spec(PinnedTool::Zizmor),
                catalog.tool_spec(PinnedTool::Opentofu),
            ]
            .as_slice(),
            "pure-tofu plan installs opentofu plus validators: {run:?}"
        );
        for key in ["MISE_RUSTUP_HOME", "MISE_CARGO_HOME", "RUSTUP_TOOLCHAIN"] {
            assert!(!env.contains_key(key), "prepare carries no {key}: {env:?}");
        }
        assert_eq!(
            env.get("MISE_NO_CONFIG").map(String::as_str),
            Some("1"),
            "isolation overlay stays: {env:?}"
        );
    }

    #[test]
    fn final_job_writes_request_before_merge() {
        let catalog = ToolCatalog::pinned();
        for acquire in [None, Some(checkout_action().expect("checkout step"))] {
            let job = final_job("ubuntu-26.04", &["rust-demo".to_owned()], acquire, &catalog)
                .expect("final job");
            assert_request_before(
                &job,
                "Merge reports",
                "write-request-v1:merge-v1",
                MERGE_OPERATION,
            );
        }
    }

    #[test]
    fn final_job_needs_plan_crates_and_lint() {
        let catalog = ToolCatalog::pinned();
        let job = final_job(
            "ubuntu-26.04",
            &["rust-demo".to_owned(), "rust-nested".to_owned()],
            None,
            &catalog,
        )
        .expect("final job");
        assert_eq!(
            job.needs,
            [
                PLAN_JOB_ID.to_owned(),
                "rust-demo".to_owned(),
                "rust-nested".to_owned(),
                LINT_JOB_ID.to_owned(),
            ]
        );
        let job = final_job("ubuntu-26.04", &[], None, &catalog).expect("final job");
        assert_eq!(job.needs, [PLAN_JOB_ID.to_owned(), LINT_JOB_ID.to_owned()]);
    }
}

//! Matrix task-job named steps: order, no-op reports, timings report.
//!
//! Task-execution contract §1 fixes twelve named steps per matrix job; a
//! step may be a validated no-op but must write an explanatory report.
//! Names are workflow-topology labels; argv always arrives validated from
//! the orchestrator, which owns stack commands. No-op reports use the
//! cache-contract §3 schema-1 task shape (`not_selected` plus the reason
//! enum); the task contract's `not_applicable` wording maps to that enum.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, NotSelectedReason, Step};

use crate::{RenderError, steps};

/// `Prepare pinned tools` step name.
pub const PREPARE_TOOLS_NAME: &str = "Prepare pinned tools";
/// `Verify toolchain` step name.
pub const VERIFY_TOOLCHAIN_NAME: &str = "Verify toolchain";
/// `Restore Cargo sources` step name.
pub const RESTORE_SOURCES_NAME: &str = "Restore Cargo sources";
/// `Restore compiler objects` step name.
pub const RESTORE_OBJECTS_NAME: &str = "Restore compiler objects";
/// `Verify prepared inputs` step name.
pub const VERIFY_INPUTS_NAME: &str = "Verify prepared inputs";
/// `Clippy` step name.
pub const CLIPPY_NAME: &str = "Clippy";
/// `Build test executables` step name.
pub const BUILD_TEST_NAME: &str = "Build test executables";
/// `Unit and integration tests` step name.
pub const TEST_NAME: &str = "Unit and integration tests";
/// `Doctests` step name.
pub const DOCTESTS_NAME: &str = "Doctests";
/// `Documentation` step name.
pub const DOCUMENTATION_NAME: &str = "Documentation";
/// `Save eligible caches` step name.
pub const SAVE_CACHES_NAME: &str = "Save eligible caches";
/// `Report timings and reuse` step name.
pub const REPORT_TIMINGS_NAME: &str = "Report timings and reuse";

/// Twelve contract named steps in execution order (contract §1 numbers
/// 1-9, 11-13; there is no step 10).
pub const TASK_STEP_NAMES: [&str; 12] = [
    PREPARE_TOOLS_NAME,
    VERIFY_TOOLCHAIN_NAME,
    RESTORE_SOURCES_NAME,
    RESTORE_OBJECTS_NAME,
    VERIFY_INPUTS_NAME,
    CLIPPY_NAME,
    BUILD_TEST_NAME,
    TEST_NAME,
    DOCTESTS_NAME,
    DOCUMENTATION_NAME,
    SAVE_CACHES_NAME,
    REPORT_TIMINGS_NAME,
];

/// Env key carrying the leg's matrix key into report scripts.
pub const NOOP_MATRIX_KEY_ENV: &str = "VELNOR_MATRIX_KEY";
/// Env key carrying the leg's matrix ID (`matrix.id`) into scripts.
pub const NOOP_MATRIX_ID_ENV: &str = "VELNOR_MATRIX_ID";
/// Env key carrying the GitHub event name into report scripts.
pub const NOOP_EVENT_ENV: &str = "VELNOR_EVENT_NAME";
/// Env key carrying the leg's task ID (`matrix.task_id`) into scripts.
pub const LEG_TASK_ID_ENV: &str = "VELNOR_TASK_ID";
/// Env key carrying the leg's obligation digest into scripts.
pub const LEG_TASK_DIGEST_ENV: &str = "VELNOR_TASK_DIGEST";
/// Exec-leg alias: matrix key env (shared with no-op report scripts).
pub const LEG_MATRIX_KEY_ENV: &str = NOOP_MATRIX_KEY_ENV;
/// Exec-leg alias: matrix ID env (shared with no-op report scripts).
pub const LEG_MATRIX_ID_ENV: &str = NOOP_MATRIX_ID_ENV;
/// Report-production operation invoked by no-op steps.
pub const NOOP_REPORT_OP: &str = "write-task-report-v1";
/// Env key carrying the no-op skip reason into the report op.
pub const NOOP_REASON_ENV: &str = "VELNOR_NOT_SELECTED_REASON";
/// Env key carrying the skipped obligation's expected digest.
///
/// Deliberately distinct from [`LEG_TASK_DIGEST_ENV`]: exec steps bake the
/// leg digest into every obligation env, so aliasing would make the report
/// op read an exec digest with no reason and fail `noop_half_present` on
/// every executed obligation.
pub const NOOP_DIGEST_ENV: &str = "VELNOR_NOOP_TASK_DIGEST";
/// Env key carrying the fixed zero exit code into the report op.
pub const NOOP_EXIT_ENV: &str = "VELNOR_EXIT_CODE";

/// Typed no-op explanation: validated task identity plus reason enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoOpReport {
    /// Skipped obligation's task ID (contract charset).
    pub task_id: String,
    /// Skipped obligation's task digest (`b3-` + 64 hex).
    pub task_digest: String,
    /// Machine-readable skip reason (cache-contract §3 enum).
    pub reason: NotSelectedReason,
}

/// One named step: execute validated argv or write a no-op report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStepMode {
    /// Run orchestrator-supplied fixed argv with fixed env.
    Execute {
        /// Fixed argument vector.
        argv: Vec<String>,
        /// Fixed environment.
        env: BTreeMap<String, String>,
    },
    /// Skip and write the explanatory schema-1 task report.
    NoOp {
        /// Validated no-op identity plus reason.
        report: NoOpReport,
        /// Staged-helper path serving the report op.
        helper: String,
    },
}

/// One named-step build request: contract name plus execution mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStepSpec {
    /// Contract step name (must be one of [`TASK_STEP_NAMES`]).
    pub name: String,
    /// Execute or no-op mode.
    pub mode: TaskStepMode,
}

/// Typed report-op invocation for one no-op step.
///
/// The script carries only validated inputs (task identity, expected
/// digest, closed-vocabulary reason) into the staged helper's
/// [`NOOP_REPORT_OP`]; run key, matrix coordinates, event, and trust
/// resolve from the downloaded plan inside the op, and report bytes
/// are produced by Rust canonical JSON, never shell-composed.
/// # Errors
pub fn noop_report_script(report: &NoOpReport, helper: &str) -> Result<String, RenderError> {
    velnor_actions_contract::validate_task_id(&report.task_id).map_err(RenderError::Contract)?;
    velnor_actions_contract::validate_digest(&report.task_digest).map_err(RenderError::Contract)?;
    validate_noop_helper(helper)?;
    let reason = reason_name(report.reason);
    Ok(format!(
        "{task_env}=\"{task_id}\" {exit_env}=\"0\" {digest_env}=\"{digest}\" {reason_env}=\"{reason}\" {op_env}={op} \"{helper}\"",
        task_env = LEG_TASK_ID_ENV,
        task_id = report.task_id,
        exit_env = NOOP_EXIT_ENV,
        digest_env = NOOP_DIGEST_ENV,
        digest = report.task_digest,
        reason_env = NOOP_REASON_ENV,
        reason = reason,
        op_env = steps::INTERNAL_OP_ENV,
        op = NOOP_REPORT_OP,
    ))
}

/// Require a staged-helper path with a version-only suffix.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for unstaged paths and for
/// suffixes outside the version charset.
fn validate_noop_helper(helper: &str) -> Result<(), RenderError> {
    let Some(suffix) = helper.strip_prefix(steps::STAGED_BINARY_PREFIX) else {
        return Err(RenderError::BadCommand(format!(
            "noop_unstaged_binary:{helper}"
        )));
    };
    let version = !suffix.is_empty()
        && suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
    if version {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!(
            "noop_unstaged_binary:{helper}"
        )))
    }
}

/// One named no-op step: fixed name, staged-helper report invocation.
///
/// The name must be a contract named step; anything else fails closed so
/// no-op reports cannot masquerade under ad-hoc labels.
/// # Errors
pub fn noop_step(name: &str, report: &NoOpReport, helper: &str) -> Result<Step, RenderError> {
    if !TASK_STEP_NAMES.contains(&name) {
        return Err(RenderError::BadCommand(format!("noop_bad_name:{name}")));
    }
    let script = noop_report_script(report, helper)?;
    steps::shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )
}

/// Build all twelve named steps in contract order.
///
/// Requires exactly the twelve names in order; each spec executes its
/// validated argv or writes its no-op report. Any gap, duplicate, or
/// misordering fails closed instead of emitting a partial matrix leg.
/// # Errors
pub fn build_task_steps(specs: &[TaskStepSpec]) -> Result<Vec<Step>, RenderError> {
    if specs.len() != TASK_STEP_NAMES.len() {
        return Err(RenderError::InvalidWorkflow(format!(
            "task_steps_wrong_count:{}",
            specs.len()
        )));
    }
    let mut built = Vec::with_capacity(TASK_STEP_NAMES.len());
    for (spec, expected) in specs.iter().zip(TASK_STEP_NAMES) {
        if spec.name != expected {
            return Err(RenderError::InvalidWorkflow(format!(
                "task_steps_misordered:{expected}:{}",
                spec.name
            )));
        }
        let step = match &spec.mode {
            TaskStepMode::Execute { argv, env } => {
                steps::shell_step(&spec.name, argv.clone(), env.clone())?
            }
            TaskStepMode::NoOp { report, helper } => noop_step(&spec.name, report, helper)?,
        };
        built.push(step);
    }
    Ok(built)
}

/// Require the twelve named steps exactly once, in order.
///
/// Prelude steps (checkout, setup) may precede them; the named steps must
/// appear as an ordered subsequence with no gaps or duplicates.
/// # Errors
pub fn check_task_step_order(job: &Job) -> Result<(), RenderError> {
    let mut previous: Option<usize> = None;
    for expected in TASK_STEP_NAMES {
        let hits: Vec<usize> = job
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| step.name == expected)
            .map(|(index, _)| index)
            .collect();
        let &[only] = hits.as_slice() else {
            let code = if hits.is_empty() {
                "task_steps_missing"
            } else {
                "task_steps_duplicated"
            };
            return Err(RenderError::InvalidWorkflow(format!("{code}:{expected}")));
        };
        if previous.is_some_and(|at| only < at) {
            return Err(RenderError::InvalidWorkflow(format!(
                "task_steps_misordered:{expected}"
            )));
        }
        previous = Some(only);
    }
    Ok(())
}

/// Require `Documentation` after `Doctests` (workflow contract §3).
///
/// Rustdoc warnings run per Rust matrix entry after doctests; a leg with
/// docs before (or without) doctests fails closed.
/// # Errors
pub fn check_doc_after_doctest(job: &Job) -> Result<(), RenderError> {
    let position = |name: &str| job.steps.iter().position(|step| step.name == name);
    let (Some(doctest_at), Some(doc_at)) = (position(DOCTESTS_NAME), position(DOCUMENTATION_NAME))
    else {
        let code = if position(DOCTESTS_NAME).is_none() {
            "doctest_missing"
        } else {
            "doc_missing"
        };
        return Err(RenderError::InvalidWorkflow(code.to_owned()));
    };
    if doc_at < doctest_at {
        return Err(RenderError::InvalidWorkflow(
            "doc_before_doctest".to_owned(),
        ));
    }
    Ok(())
}

/// Final `Report timings and reuse` step over orchestrator argv.
///
/// The vector must reference `matrix-report.json`: the step writes the
/// cache-contract §3 aggregate for its leg. Per-step timing capture plus
/// step-level `always()` (via `Step.condition`, serialized by `document`)
/// are orchestrator/contract work tracked alongside this constructor.
/// # Errors
pub fn timings_report_step(
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if !argv.iter().any(|arg| arg.contains("matrix-report.json")) {
        return Err(RenderError::BadCommand(
            "timings_without_matrix_report".to_owned(),
        ));
    }
    steps::shell_step(REPORT_TIMINGS_NAME, argv, env)
}

/// Locked/offline prepared-inputs step over validated Mise argv.
///
/// The task §2 step runs after Prepare/Verify-toolchain in
/// [`TASK_STEP_NAMES`] order; argv must invoke `mise` (the locked
/// qualification vector arrives from the orchestrator).
/// # Errors
pub fn prepared_inputs_step(
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if argv.first().is_none_or(|program| program != "mise") {
        return Err(RenderError::BadCommand(
            "prepared_inputs_without_mise".to_owned(),
        ));
    }
    steps::shell_step(VERIFY_INPUTS_NAME, argv, env)
}

/// Require the timings report to close the named steps.
///
/// Non-named steps (artifact upload) may follow; no named step may run
/// after the final report.
/// # Errors
pub fn check_timings_report_last(job: &Job) -> Result<(), RenderError> {
    let mut last_named: Option<&str> = None;
    for step in &job.steps {
        if TASK_STEP_NAMES.contains(&step.name.as_str()) {
            last_named = Some(step.name.as_str());
        }
    }
    match last_named {
        None => Err(RenderError::InvalidWorkflow(
            "timings_report_missing".to_owned(),
        )),
        Some(REPORT_TIMINGS_NAME) => Ok(()),
        Some(_) => Err(RenderError::InvalidWorkflow(
            "timings_report_not_last".to_owned(),
        )),
    }
}

/// Task-contract `not_applicable` maps to this cache-schema reason.
///
/// Task-execution contract §1 reports inapplicable steps (compiler-object
/// restore on Cargo profiles, unselected doctests) as `not_applicable`;
/// the cache-contract §3 schema carries no such status, so no-op steps
/// report `not_selected` with [`NotSelectedReason::Unsupported`].
pub const NOT_APPLICABLE_REASON: NotSelectedReason = NotSelectedReason::Unsupported;

/// Task contract wording note: the cache schema has no `not_applicable`
/// status, so no-op steps report `not_selected` with this reason enum.
fn reason_name(reason: NotSelectedReason) -> &'static str {
    match reason {
        NotSelectedReason::UpstreamFailed => "upstream_failed",
        NotSelectedReason::NotInPlan => "not_in_plan",
        NotSelectedReason::Unsupported => "unsupported",
        NotSelectedReason::CancelledByPolicy => "cancelled_by_policy",
    }
}

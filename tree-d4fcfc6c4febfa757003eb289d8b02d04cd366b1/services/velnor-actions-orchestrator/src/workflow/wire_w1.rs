//! W1 emission wiring: orchestrator halves of cross-crate TODO rows.
//!
//! Self-declared from `workflow.rs` (`#[path]`, no `lib.rs` edit) so the
//! integrator only registers the companion test file. Every helper here
//! answers one anchored EMIT entry: checkout provenance, step-syntax
//! vetting, the plan-job workspace Format step, MBX gating, the Gate-6
//! task-cache gate, and the V1 actionlint variable set.

use std::collections::BTreeMap;

use velnor_actions_actionlint::{
    ActionlintCapabilities, PinnedActionRef, StepSyntax,
    actions::{CACHE_ACTION_SHA, CACHE_ACTION_VERSION},
    checkout_inputs_schema, validate_action_inputs,
};
use velnor_actions_contract_planning::ProposedTask;
use velnor_actions_contract_workflow::{Job, Step, StepKind, StepRole};
use velnor_actions_mise::{Gate6Fixture, TaskCacheMode, ToolCatalog, ToolHomes};
use velnor_actions_rust::is_workspace_fmt_task;
use velnor_actions_workflow_renderer::cache_steps::{
    CompileDriver, TASK_ARTIFACTS_DIR, cache_action_step, check_mbx_gating,
};
use velnor_actions_workflow_renderer::plan_format;
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::utf8::strings_of_env;

/// Declared repository configuration variable names (GEN-2.14).
///
/// V1 emits no `vars:` references, so the exact declared set is empty;
/// the renderer-side set threads through this single constructor when a
/// future workflow declares variables.
#[must_use]
pub(crate) fn declared_config_variables() -> Vec<String> {
    Vec::new()
}

/// Pinned checkout action without persisted credentials (WF-3.50).
///
/// The `uses:` value comes from the canonical actionlint ref and the
/// inputs validate against the checkout schema before construction, so
/// generation aborts pre-write on unknown/missing/empty inputs.
///
/// # Errors
///
/// Returns a contract error when the fixed inputs fail schema validation.
pub(crate) fn checkout_step() -> Result<Step, OrchestratorError> {
    let with = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    validate_action_inputs(&checkout_inputs_schema(), &with).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(Step {
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: PinnedActionRef::checkout().uses_value(),
            with,
            env: BTreeMap::new(),
        },
    })
}

/// Checkout with full history for git-archaeology jobs (plan only).
///
/// Plan verifies the checkout against the PR head through the merge
/// commit's second parent and diffs `base...head` for affected work;
/// both need history the default depth-1 checkout never fetches, so a
/// shallow plan checkout fails closed with `checkout_head_mismatch` on
/// every pull-request run (CI run 36749240499).
///
/// # Errors
///
/// Returns a contract error when the fixed inputs fail schema validation.
pub(crate) fn checkout_step_full() -> Result<Step, OrchestratorError> {
    let with = BTreeMap::from([
        ("persist-credentials".to_owned(), "false".to_owned()),
        ("fetch-depth".to_owned(), "0".to_owned()),
    ]);
    validate_action_inputs(&checkout_inputs_schema(), &with).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    Ok(Step {
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: PinnedActionRef::checkout().uses_value(),
            with,
            env: BTreeMap::new(),
        },
    })
}

/// Reject workflow syntax the pinned actionlint cannot parse (PAR-6.1).
///
/// Native step parallelism stays unqualified, so any future native-key
/// emission fails here.
///
/// # Errors
///
/// Returns an actionlint error for unqualified syntax.
pub(crate) fn vet_step_syntax(syntax: StepSyntax) -> Result<(), OrchestratorError> {
    ActionlintCapabilities::for_pinned()
        .check_step_syntax(syntax)
        .map_err(OrchestratorError::from)
}

/// Task-layer restore/save pair, gated on Gate-6 qualification.
///
/// Emits the restore step before and the save step after the
/// obligation payload only with a qualification fixture and a live
/// cache mode; release legs (`Off`) and unqualified generation emit
/// neither.
///
/// # Errors
///
/// Returns actionlint/render errors for rejected pins or step shapes.
pub(crate) fn maybe_task_cache_steps(
    fixture: Option<&Gate6Fixture>,
    mode: TaskCacheMode,
    key: &str,
) -> Result<Vec<Step>, OrchestratorError> {
    if fixture.is_none() || mode == TaskCacheMode::Off {
        return Ok(Vec::new());
    }
    let restore_uses = PinnedActionRef::new(
        "actions/cache",
        Some("restore"),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )?
    .uses_value();
    let save_uses = PinnedActionRef::new(
        "actions/cache",
        Some("save"),
        CACHE_ACTION_SHA,
        CACHE_ACTION_VERSION,
    )?
    .uses_value();
    let paths = vec![TASK_ARTIFACTS_DIR.to_owned()];
    Ok(vec![
        cache_action_step(true, &restore_uses, "task", key, &[], &paths)?,
        cache_action_step(false, &save_uses, "task", key, &[], &paths)?,
    ])
}

/// Display name of the plan-job deferred format-report step.
pub(crate) const REPORT_FORMAT_NAME: &str = "Report Format";

/// Plan-job `Format` step for the workspace formatting scope only.
///
/// Root cause (P05-5): the plan job duplicated the first crate's
/// formatting and synthesized an overlapping whole-workspace format,
/// so one scope had three owners. Per-package formatting lives in
/// crate jobs; this returns a step only for a package-less workspace
/// `Fmt` obligation (explicit root `rustfmt` config), the one distinct
/// scope the plan job owns. Derivation (`derive_for_config`) suppresses
/// the workspace task whenever per-package `Fmt` tasks exist for the
/// same config, so this step never re-checks crate-owned files (R28).
/// No synthesis, no fallback.
///
/// The step saves its exit code to an outcome file instead of
/// reporting directly: the workflow contract fixes `Format` before
/// `Plan`, so the plan the report binds does not exist yet. The
/// post-plan steps from [`workspace_format_report_steps`] report it.
///
/// # Errors
///
/// Returns contract/render errors for rejected vectors or step shapes.
pub(crate) fn workspace_format_step(
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<Option<Step>, OrchestratorError> {
    let Some(fmt) = workspace_fmt_group(discovery) else {
        return Ok(None);
    };
    let argv = crate::vectors::task_argv(fmt, catalog)?;
    if argv.first().is_none_or(|program| program != "mise") {
        return Err(OrchestratorError::Contract {
            problem: "format_without_mise".to_owned(),
        });
    }
    let joined = velnor_actions_workflow_steps::join_argv_for_run(&argv).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    let matrix_key = matrix_key_for(fmt)?;
    let outcome = crate::matrix_step::outcome_path_for_key(&matrix_key);
    let start = crate::matrix_step::start_path_for_key(&matrix_key);
    let run = crate::matrix_step::outcome_wrapper_argv(&joined, &outcome, &start);
    let env = format_step_env(catalog)?;
    plan_format::format_step(run, &env)
        .map(Some)
        .map_err(OrchestratorError::from)
}

/// Post-plan report steps for the workspace `Fmt` obligation.
///
/// Reports the saved `Format` outcome through the staged helper once
/// the plan exists, then uploads the plan job's one crate artifact.
/// Empty when the plan job owns no format scope.
///
/// # Errors
///
/// Returns contract/render errors for rejected vectors or step shapes.
pub(crate) fn workspace_format_report_steps(
    discovery: &Discovery,
) -> Result<Vec<Step>, OrchestratorError> {
    let Some(fmt) = workspace_fmt_group(discovery) else {
        return Ok(Vec::new());
    };
    let matrix_key = matrix_key_for(fmt)?;
    let outcome = crate::matrix_step::outcome_path_for_key(&matrix_key);
    let start = crate::matrix_step::start_path_for_key(&matrix_key);
    let helper = crate::matrix_step::helper_path_for_version();
    let report = velnor_actions_workflow_steps::shell_step(
        REPORT_FORMAT_NAME,
        crate::matrix_step::deferred_report_argv(&outcome, &helper, &start),
        BTreeMap::from([(
            crate::task_report::TASK_ID_ENV.to_owned(),
            fmt.task_id.clone(),
        )]),
    )
    .map_err(OrchestratorError::from)?;
    let upload = crate::matrix_step::crate_upload_step(PLAN_JOB_ID)?;
    Ok(vec![report, upload])
}

/// Package-less workspace `Fmt` task, when the plan job owns one.
fn workspace_fmt_group(discovery: &Discovery) -> Option<&ProposedTask> {
    discovery.proposals.iter().find(|task| {
        is_workspace_fmt_task(&task.task_kind, &task.identity.unit_id, &task.display_name)
    })
}

/// Stable matrix key for one workspace task.
fn matrix_key_for(task: &ProposedTask) -> Result<String, OrchestratorError> {
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group(&task.stack_id, &task.task_id)
            .map_err(crate::internal::internal_contract)?;
    velnor_actions_contract::matrix_key_for_id(&matrix_id)
        .map_err(crate::internal::internal_contract)
}

/// Full `exec` verification env routing Format at the prepared toolchain.
///
/// Without the owned homes the step resolves whatever ambient toolchain
/// the runner offers (a minimal image toolchain has no `rustfmt`); with
/// them it runs the `Prepare pinned tools` toolchain, and a missing tool
/// fails as a preparation error instead of installing.
fn format_step_env(catalog: &ToolCatalog) -> Result<BTreeMap<String, String>, OrchestratorError> {
    strings_of_env(&ToolHomes::runner_temp().exec_env(catalog))
        .map_err(|problem| OrchestratorError::Contract { problem })
}

/// Gate MBX presence in crate jobs against their driver selection.
///
/// Cargo crates must be MBX-free; MBX crates carry exactly one
/// objects-mode step each. Jobs outside the driver map are unchecked.
///
/// # Errors
///
/// Returns a render error when MBX presence mismatches the driver.
pub(crate) fn check_crate_mbx_gating(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
) -> Result<(), OrchestratorError> {
    check_mbx_gating(jobs, drivers).map_err(OrchestratorError::from)
}

#[cfg(test)]
mod tests;

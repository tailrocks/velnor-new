//! Compare the compiled Rust frame with the plan and fresh source before execution.

use std::path::PathBuf;

use velnor_actions_contract::{MatrixEntry, Plan, parse_strict_json};

use crate::OrchestratorError;
use crate::current_semantic_input_proof::CurrentSemanticInputProof;
use crate::internal::internal;
use crate::internal::plan_obligation::source_identity::ResolvedSourceIdentity;

/// Private operation called by the compiled ordinary Rust report wrapper.
pub const RUST_REPORT_PREEXEC_OP: &str = "validate-rust-report-preexec-v1";
/// Original compiler argv, bound by the compiled owner's environment.
pub(crate) const FRAME_ARGV_ENV: &str = "VELNOR_RUST_FRAME_ARGV_JSON";
/// Original compiler toolchain, bound by the compiled owner's environment.
pub(crate) const FRAME_TOOLCHAIN_ENV: &str = "VELNOR_RUST_FRAME_TOOLCHAIN";

/// Admit the original compiler frame only against independently acquired source.
/// # Errors
/// Refuses malformed frames, covered tasks, stale plans, and missing source authority.
pub fn validate_rust_report_preexec() -> Result<(), OrchestratorError> {
    let run = crate::internal_request::resolve_run_key(None)?;
    let task = required_env(crate::task_report::TASK_ID_ENV)?;
    let digest = required_env(crate::matrix_step::OBLIGATION_TASK_DIGEST_ENV)?;
    let argv_json = required_env(FRAME_ARGV_ENV)?;
    if argv_json.len() > velnor_actions_contract::SOURCE_HELPER_ARGUMENT_BYTES_MAX {
        return Err(internal("rust_frame_argument_size"));
    }
    let value = parse_strict_json(&argv_json).map_err(|_| internal("rust_frame_argv_invalid"))?;
    let argv: Vec<String> =
        serde_json::from_value(value).map_err(|_| internal("rust_frame_argv_invalid"))?;
    let toolchain = required_env(FRAME_TOOLCHAIN_ENV)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let plan = crate::task_report::load_plan(&run, &temp)?;
    validate_compiled_frame(&plan, &task, &digest, &argv, &toolchain)?;
    let (entry, _) = crate::task_report::entry_and_digest(&plan, &task)?;
    let proof = CurrentSemanticInputProof::acquire_for_execution(&plan, entry)?;
    if !crate::rust_report_wrapper::requires_compiler(proof.task())
        || proof.task().task_id != task
        || proof.task().stack_id != entry.stack_id
        || proof.run_key() != plan.run_key
        || proof.runner_label() != plan.runner.label
        || proof.generator().version != plan.generator.version
        || proof.generator().target != plan.generator.target
        || proof.generator().sha256 != plan.generator.sha256
        || !proof.root().is_absolute()
    {
        return Err(internal("rust_frame_source_context_mismatch"));
    }
    validate_current_identity(&plan, entry, &digest, &argv, &toolchain, proof.identity())
}

/// Frame and plan comparisons precede every disposition shortcut.
fn validate_compiled_frame(
    plan: &Plan,
    task: &str,
    digest: &str,
    argv: &[String],
    toolchain: &str,
) -> Result<(), OrchestratorError> {
    crate::task_report::validate_expected_digest(plan, task, digest)?;
    if !crate::rust_report_wrapper::requires_compiler_task(task)
        || argv.is_empty()
        || argv
            .iter()
            .any(|arg| arg.is_empty() || arg.chars().any(char::is_control))
        || toolchain.is_empty()
        || toolchain.chars().any(char::is_control)
    {
        return Err(internal("rust_frame_recipe_invalid"));
    }
    let actual = crate::internal::plan_obligation::task_digest(task, argv, toolchain, None, None)?;
    if actual != digest {
        return Err(internal("rust_frame_recipe_digest_mismatch"));
    }
    if crate::covered_tasks::covered_by_baseline(plan, task) {
        return Err(internal("rust_frame_execution_covered"));
    }
    let (entry, _) = crate::task_report::entry_and_digest(plan, task)?;
    validate_ordinary_metadata(&entry.adapter_metadata)?;
    if entry.native_recipe.is_some() {
        return Err(internal("rust_frame_foreign_descriptor"));
    }
    Ok(())
}

/// Use the existing canonical proposal resolver; serialized descriptors grant nothing.
fn validate_current_identity(
    plan: &Plan,
    entry: &MatrixEntry,
    digest: &str,
    argv: &[String],
    toolchain: &str,
    actual: &ResolvedSourceIdentity,
) -> Result<(), OrchestratorError> {
    let obligation = plan
        .obligations
        .iter()
        .find(|ob| ob.task_id == entry.task_id)
        .ok_or_else(|| internal("task_without_obligation"))?;
    let joined = velnor_actions_workflow_renderer::join_argv_for_run(&actual.argv)?;
    validate_ordinary_metadata(&entry.adapter_metadata)?;
    if actual.argv != argv
        || actual.toolchain_id != toolchain
        || actual.task_digest != digest
        || entry.task_digest != actual.task_digest
        || entry.run != joined
        || entry.input_digest != actual.input_digest
        || obligation.input_digest != actual.input_digest
        || obligation.closure_digest != actual.closure_digest
        || obligation.execution_identity != actual.execution_identity
        || entry.native_recipe != actual.native_recipe
        || actual.helper_obligation.is_some()
        || actual.helper_record.is_some()
    {
        return Err(internal("rust_frame_current_identity_mismatch"));
    }
    Ok(())
}

/// Only an object with no helper descriptor represents an ordinary Rust frame.
fn validate_ordinary_metadata(metadata: &serde_json::Value) -> Result<(), OrchestratorError> {
    let object = metadata
        .as_object()
        .ok_or_else(|| internal("rust_frame_metadata_invalid"))?;
    if object.contains_key("helper_obligation") {
        return Err(internal("rust_frame_foreign_descriptor"));
    }
    Ok(())
}

fn required_env(key: &str) -> Result<String, OrchestratorError> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_rust_frame_environment"))
}

#[cfg(test)]
#[path = "rust_report_preexec_tests.rs"]
mod tests;

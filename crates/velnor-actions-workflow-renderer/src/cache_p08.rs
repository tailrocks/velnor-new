//! V2 tools-cache identity and the shared Cargo source-cache contract.
//!
//! Tool archives use typed pins and runtime-qualified paths. Cargo
//! sources remain an independent cache layer; MBX owns compiler objects.

use velnor_actions_contract::{Job, Step, StepKind};

use crate::{MiseSetup, RenderError, setup};

#[path = "cache_p08_tool_payload.rs"]
mod tool_payload;
pub use tool_payload::{ToolsCacheInputs, ToolsCachePayload};
pub(crate) use tool_payload::{
    runtime_identity_action_file, runtime_identity_action_uses, runtime_identity_script_file,
    runtime_prelude_action_file, runtime_prelude_action_uses, validate_runtime_identity_action,
};
#[path = "cache_p08_save_policy.rs"]
mod save_policy;
#[path = "cache_p08_setup.rs"]
mod setup_pipeline;

pub use crate::cache_elect::elect_cache_writers;

/// Display name of the shared sources restore step.
pub const RESTORE_SOURCES_NAME: &str = "Restore Cargo sources";
/// Display name of the shared sources save step.
pub const SAVE_SOURCES_NAME: &str = "Save Cargo sources";
/// Display name of the runtime identity step gating the tools cache.
pub const TOOLS_CACHE_IDENTITY_NAME: &str = "V2 identity";
/// Step output owner used by both V2 restore and save expressions.
pub const TOOLS_CACHE_IDENTITY_STEP_ID: &str = "v2";
/// Canonical key expression; the output hashes both static tools and runtime identity.
pub(crate) const TOOLS_CACHE_KEY_EXPRESSION: &str = "mise-tools-v2-${{steps.v2.outputs.identity}}";
/// Composite-action input carrying the V2 static tools digest.
pub(crate) const TOOLS_CACHE_IDENTITY_DIGEST_INPUT: &str =
    velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
/// Cache restore is unavailable unless runtime roots/image were qualified.
pub const TOOLS_CACHE_RESTORE_CONDITION: &str = "steps.v2.outputs.enabled == 'true'";
/// Cache writes require an eligible trusted producer and qualified identity.
pub(crate) fn tools_cache_save_condition() -> String {
    save_policy::condition()
}

/// Insert pinned Mise setup and a V2 tools-cache restore into one job.
/// # Errors
pub(crate) fn ensure_tools_cache_v2(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    setup_pipeline::ensure_tools_cache_v2(job_id, job, setup, always, target, checkout_uses)
}

/// Reject the retired broad `rust-cache` archive in generated jobs.
/// # Errors
pub fn check_no_legacy_rust_cache(job_id: &str, job: &Job) -> Result<(), RenderError> {
    if job.steps.iter().any(|step| {
        matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("Swatinem/rust-cache@"))
    }) {
        return Err(RenderError::InvalidWorkflow(format!(
            "legacy_rust_cache:{job_id}"
        )));
    }
    Ok(())
}

/// Require MBX object restore before every Cargo source fetch step.
/// # Errors
pub fn check_mbx_before_fetch(job_id: &str, job: &Job) -> Result<(), RenderError> {
    let at = |role| job.steps.iter().position(|step| step.role == Some(role));
    let mbx = at(velnor_actions_contract::StepRole::MbxCache);
    let fetch = job
        .steps
        .iter()
        .position(|step| step.role == Some(velnor_actions_contract::StepRole::CargoSourcesFetch));
    if let (Some(mbx), Some(fetch_at)) = (mbx, fetch)
        && fetch_at < mbx
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "fetch_before_mbx:{job_id}"
        )));
    }
    Ok(())
}

/// Union exact Mise selectors from each shell step's fixed argv.
#[must_use]
pub fn infer_job_tools(job: &Job) -> Vec<String> {
    let mut specs = std::collections::BTreeSet::new();
    for step in &job.steps {
        let StepKind::Shell { run, .. } = &step.kind else {
            continue;
        };
        let mut take = false;
        for word in crate::cache_p08_detect::detector_words(run) {
            if word == "install" || word == "exec" {
                take = true;
                continue;
            }
            if word == "--" {
                take = false;
                continue;
            }
            if take && is_tool_spec(&word) {
                specs.insert(word);
            }
        }
    }
    specs.into_iter().collect()
}

/// True for catalog version spellings (`2026.10.4`); never `latest`.
pub(crate) fn is_catalog_version(value: &str) -> bool {
    !value.is_empty()
        && value != "latest"
        && !value.contains("latest")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
        && value.contains('.')
        && !value.contains("${{")
}

/// True for `<tool>@<version>` selectors supported by the tool cache.
pub(crate) fn is_tool_spec(value: &str) -> bool {
    let Some((tool, version)) = value.split_once('@') else {
        return false;
    };
    !tool.is_empty()
        && !version.is_empty()
        && !value.contains(' ')
        && !value.contains('\n')
        && tool.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b':' | b'/' | b'-' | b'_' | b'.' | b'[' | b']' | b'=' | b','
                )
        })
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
}

/// Detect the current pinned Setup Mise shape, with cache delegated to V2.
pub(crate) fn setup_step(setup: &MiseSetup) -> Result<Step, RenderError> {
    setup::mise_setup_step(setup)
}

/// Compare semantic step fields while leaving the display-only name mutable.
pub(crate) fn same_step_semantics(left: &Step, right: &Step) -> bool {
    left.id == right.id
        && left.role == right.role
        && left.condition == right.condition
        && left.kind == right.kind
}

/// True only for the canonical typed V2 cache-key expression.
pub(crate) fn is_v2_cache_key_expression(value: &str) -> bool {
    value == TOOLS_CACHE_KEY_EXPRESSION
}

/// Validate the component-install payload against the resolved toolchain.
pub(crate) fn rust_components(
    step: &Step,
    version: &str,
    target: &str,
) -> Result<Vec<String>, RenderError> {
    setup_pipeline::rust_components(step, version, target)
}

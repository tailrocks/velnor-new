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
pub(crate) use setup_pipeline::move_acquire_after_checkout;

pub use crate::cache_elect::{elect_cache_writers, elect_cache_writers_with_policy};

/// Display name of the shared sources restore step.
pub const RESTORE_SOURCES_NAME: &str = "Restore Cargo sources";
/// Display name of the shared sources save step.
pub const SAVE_SOURCES_NAME: &str = "Save Cargo sources";
/// Display name of the runtime identity step gating the tools cache.
pub const TOOLS_CACHE_IDENTITY_NAME: &str = "V2 identity";
/// Step output owner used by both V2 restore and save expressions.
pub const TOOLS_CACHE_IDENTITY_STEP_ID: &str = "v2";
/// Canonical key expression; the output hashes both static tools and runtime identity.
pub(crate) const TOOLS_CACHE_KEY_EXPRESSION: &str =
    "mise-tools-v2-payload-v3-${{steps.v2.outputs.identity}}";
/// Composite-action input carrying the V2 static tools digest.
pub(crate) const TOOLS_CACHE_IDENTITY_DIGEST_INPUT: &str =
    velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
/// Condition for cache restore when a job needs the plan job.
pub const TOOLS_CACHE_NORMAL_RUN_CONDITION: &str =
    "needs.plan.result == 'success' && needs.plan.outputs.qualification_campaign == ''";
/// Jobs that do not need the plan job cannot read its outputs.
pub const TOOLS_CACHE_PLAN_RUN_CONDITION: &str = "github.event_name != 'workflow_dispatch'";
/// A tool seed may run only after its runtime identity action succeeds.
pub const TOOLS_CACHE_IDENTITY_CONDITION: &str = "steps.v2.outputs.enabled == 'true'";
/// Restore only in a normal planned run with a qualified runtime identity.
pub const TOOLS_CACHE_RESTORE_CONDITION: &str = "needs.plan.result == 'success' && needs.plan.outputs.qualification_campaign == '' && steps.v2.outputs.enabled == 'true'";
/// These jobs use the event condition and the identity from their own run.
pub const TOOLS_CACHE_PLAN_RESTORE_CONDITION: &str =
    "github.event_name != 'workflow_dispatch' && steps.v2.outputs.enabled == 'true'";

/// This configuration controls whether setup steps can use cache actions.
#[derive(Clone, Copy)]
pub(crate) struct ToolsCacheRenderOptions<'a> {
    pub target: &'a str,
    pub scale_set_image_source_sha: Option<&'a str>,
    pub checkout_uses: &'a str,
    pub require_verified_helper: bool,
}

pub(crate) fn tools_cache_identity_condition(needs_plan: bool) -> &'static str {
    if needs_plan {
        TOOLS_CACHE_NORMAL_RUN_CONDITION
    } else {
        TOOLS_CACHE_PLAN_RUN_CONDITION
    }
}

pub(crate) fn tools_cache_restore_condition(needs_plan: bool) -> &'static str {
    if needs_plan {
        TOOLS_CACHE_RESTORE_CONDITION
    } else {
        TOOLS_CACHE_PLAN_RESTORE_CONDITION
    }
}
/// Cache writes require an eligible producer under `policy` and qualified identity.
pub(crate) fn tools_cache_save_condition(
    policy: velnor_actions_contract::PullRequestCachePolicy,
) -> String {
    save_policy::condition(policy)
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
    ensure_tools_cache_v2_with_source(job_id, job, setup, always, target, None, checkout_uses)
}

/// Insert V2 tools-cache setup with the validated immutable Scale Set image source.
/// # Errors
pub(crate) fn ensure_tools_cache_v2_with_source(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    target: &str,
    scale_set_image_source_sha: Option<&str>,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    ensure_tools_cache_v2_with_helper_policy(
        job_id,
        job,
        setup,
        always,
        ToolsCacheRenderOptions {
            target,
            scale_set_image_source_sha,
            checkout_uses,
            require_verified_helper: true,
        },
    )
}

/// Put the tools-cache steps in the job when its settings require the verified Velnor program.
///
/// If a source build has no verified Velnor program, keep setup steps. Leave out
/// cache actions only when its settings allow this.
/// # Errors
pub(crate) fn ensure_tools_cache_v2_with_helper_policy(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    options: ToolsCacheRenderOptions<'_>,
) -> Result<(), RenderError> {
    setup_pipeline::ensure_tools_cache_v2(job_id, job, setup, always, options)
}

pub(crate) fn ensure_mise_setup_only(
    job_id: &str,
    job: &mut Job,
    setup: &MiseSetup,
    always: bool,
    checkout_uses: &str,
) -> Result<(), RenderError> {
    setup_pipeline::ensure_mise_setup_only(job_id, job, setup, always, checkout_uses)
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

/// True for catalog version spellings (`2026.10.13`); never `latest`.
pub(crate) fn is_catalog_version(value: &str) -> bool {
    if velnor_actions_contract::config::project_tool_refs::is_project_tool_ref(value) {
        return true;
    }
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
        && (velnor_actions_contract::config::project_tool_refs::is_project_tool_ref(version)
            || version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+')))
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

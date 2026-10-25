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

pub use crate::cache_elect::{elect_cache_writers, elect_cache_writers_with_policy};

pub const RESTORE_SOURCES_NAME: &str = "Restore Cargo sources";
pub const SAVE_SOURCES_NAME: &str = "Save Cargo sources";
pub const TOOLS_CACHE_IDENTITY_NAME: &str = "V2 identity";
pub const TOOLS_CACHE_IDENTITY_STEP_ID: &str = "v2";
pub(crate) const TOOLS_CACHE_KEY_EXPRESSION: &str = "mise-tools-v2-${{steps.v2.outputs.identity}}";
pub(crate) const TOOLS_CACHE_IDENTITY_DIGEST_INPUT: &str =
    velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
pub const TOOLS_CACHE_RESTORE_CONDITION: &str = "steps.v2.outputs.enabled == 'true'";
pub(crate) fn tools_cache_save_condition(
    policy: velnor_actions_contract::PullRequestCachePolicy,
) -> String {
    save_policy::condition(policy)
}

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

pub(crate) fn setup_step(setup: &MiseSetup) -> Result<Step, RenderError> {
    setup::mise_setup_step(setup)
}

pub(crate) fn same_step_semantics(left: &Step, right: &Step) -> bool {
    left.id == right.id
        && left.role == right.role
        && left.condition == right.condition
        && left.kind == right.kind
}

pub(crate) fn is_v2_cache_key_expression(value: &str) -> bool {
    value == TOOLS_CACHE_KEY_EXPRESSION
}

pub(crate) fn rust_components(
    step: &Step,
    version: &str,
    target: &str,
) -> Result<Vec<String>, RenderError> {
    setup_pipeline::rust_components(step, version, target)
}

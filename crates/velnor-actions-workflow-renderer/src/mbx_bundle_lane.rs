//! Choose one MBX cache transport owner for each runner lane.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::RenderError;
use crate::cache_steps::{MBX_ACTION_NAME, MBX_CACHE_MODE_ENV, MBX_PREFLIGHT_NAME, is_mbx_action};

use super::{
    MBX_BUNDLE_EXPORT_NAME, MBX_BUNDLE_IMPORT_NAME, MBX_BUNDLE_KEY_NAME, MBX_BUNDLE_RESTORE_NAME,
    MBX_BUNDLE_SAVE_NAME, MBX_STORE_INIT_NAME, PREP_IF, SAVE_IF, store,
};

const HOSTED_BACKEND: &str = "${{ runner.environment == 'github-hosted' && 'github' || 'local' }}";
const PUSH_HOSTED_CACHE_MODE: &str = "${{ runner.environment == 'github-hosted' && github.event_name == 'push' && 'write' || 'read' }}";

/// Keep native GitHub caching on hosted runners and use Velnor's bundle on Scale Set.
pub(super) fn configure_action_transport(steps: &mut [Step]) {
    for step in steps {
        let StepKind::Action { uses, with, env } = &mut step.kind else {
            continue;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            continue;
        }
        with.insert("backend".to_owned(), HOSTED_BACKEND.to_owned());
        env.insert(
            MBX_CACHE_MODE_ENV.to_owned(),
            PUSH_HOSTED_CACHE_MODE.to_owned(),
        );
    }
}

/// Set up a private store only when Velnor owns the Scale Set bundle lane.
pub(super) fn insert_private_store_init(job: &mut Job) -> Result<(), RenderError> {
    if job
        .steps
        .iter()
        .any(|step| step.name == MBX_STORE_INIT_NAME)
    {
        return Ok(());
    }
    let at = job
        .steps
        .iter()
        .position(|step| step.name == MBX_PREFLIGHT_NAME)
        .or_else(|| job.steps.iter().position(is_mbx_action))
        .ok_or_else(|| RenderError::InvalidWorkflow("mbx_store_init_without_action".to_owned()))?;
    let mut step = crate::steps::shell_step(
        MBX_STORE_INIT_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            store::STORE_INIT_SCRIPT.to_owned(),
        ],
        BTreeMap::new(),
    )?;
    step.condition = Some(store::SCALE_SET_ONLY_IF.to_owned());
    job.steps.insert(at, step);
    Ok(())
}

/// Scope every Velnor bundle operation to the Scale Set lane.
pub(super) fn scope_bundle_route_to_scale_set(job: &mut Job) {
    let export_condition = format!("{} && {PREP_IF}", store::SCALE_SET_ONLY_IF);
    let save_condition = format!("{} && {SAVE_IF}", store::SCALE_SET_ONLY_IF);
    for step in &mut job.steps {
        match step.name.as_str() {
            MBX_BUNDLE_KEY_NAME => {
                step.condition = Some(store::SCALE_SET_ONLY_IF.to_owned());
            }
            MBX_BUNDLE_RESTORE_NAME | MBX_BUNDLE_IMPORT_NAME => {
                let condition = step.condition.as_deref().unwrap_or("success()");
                step.condition = Some(format!("{} && ({condition})", store::SCALE_SET_ONLY_IF));
            }
            MBX_BUNDLE_EXPORT_NAME => step.condition = Some(export_condition.clone()),
            MBX_BUNDLE_SAVE_NAME => step.condition = Some(save_condition.clone()),
            _ => {}
        }
    }
}

/// Build the exact upstream object-cache generation for the pinned MBX version.
pub(super) fn directory_cache_generation(generation: &str) -> Result<String, RenderError> {
    let version = generation
        .strip_prefix("velnor-mbx-")
        .ok_or_else(|| RenderError::InvalidWorkflow("bad_mbx_cache_generation".to_owned()))?;
    let mut parts = version.split('.');
    let major = parse_version_part(parts.next())?;
    let minor = parse_version_part(parts.next())?;
    let _patch = parse_version_part(parts.next())?;
    if parts.next().is_some() {
        return Err(RenderError::InvalidWorkflow(
            "bad_mbx_cache_generation".to_owned(),
        ));
    }
    if major > 1 || (major == 1 && minor >= 12) {
        Ok(format!("{generation}-dir"))
    } else {
        Ok(generation.to_owned())
    }
}

fn parse_version_part(part: Option<&str>) -> Result<u64, RenderError> {
    part.filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| RenderError::InvalidWorkflow("bad_mbx_cache_generation".to_owned()))
}

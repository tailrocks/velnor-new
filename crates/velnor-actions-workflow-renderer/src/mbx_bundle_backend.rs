//! Validate job-owned MBX inputs and pin the action to its local backend.

use velnor_actions_contract::{Job, StepKind};

use crate::RenderError;
use crate::cache_steps::MBX_ACTION_NAME;
use crate::mbx_bundle::{MBX_RESOURCE_EVIDENCE_REQUIRED_ENV, MBX_SCOPE_INPUT, MBX_WRITER_INPUT};

/// These names remain job-owned so later steps cannot redirect MBX or export.
pub(super) fn reject_private_env_overrides(job: &Job, id: &str) -> Result<(), RenderError> {
    for step in &job.steps {
        let env = match &step.kind {
            StepKind::Action { env, .. } | StepKind::Shell { env, .. } => env,
            StepKind::Internal { .. } => continue,
        };
        if [
            "MBX_CACHE_DIR",
            "MBX_TARGET_ROOT",
            "MBX_SHIMS_DIR",
            "MBX_CACHE_EXPORT_GROUP",
            MBX_RESOURCE_EVIDENCE_REQUIRED_ENV,
        ]
        .iter()
        .any(|name| env.contains_key(*name))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "mbx_private_env_override:{id}:{}",
                step.name
            )));
        }
    }
    Ok(())
}

/// Prevent the stock GitHub backend from restoring/importing/saving internally.
pub(super) fn pin_local_backend(job: &mut Job, id: &str) -> Result<(), RenderError> {
    for step in &mut job.steps {
        let StepKind::Action { uses, with, env } = &mut step.kind else {
            continue;
        };
        if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
            continue;
        }
        if ["isolate-objects-cache", "cache-key-suffix"]
            .iter()
            .any(|input| with.contains_key(*input))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "unsupported_mbx_input:{id}"
            )));
        }
        with.insert("backend".to_owned(), "local".to_owned());
        for input in [
            MBX_SCOPE_INPUT,
            MBX_WRITER_INPUT,
            "github-cache-mode",
            "cache-generation",
            "cache-key",
            "restore-keys",
            "save-on-workflow-dispatch",
            "save-on-pull-request",
            "save-on-protected-branch",
        ] {
            with.remove(input);
        }
        env.remove("ACTIONS_CACHE_MODE");
    }
    Ok(())
}

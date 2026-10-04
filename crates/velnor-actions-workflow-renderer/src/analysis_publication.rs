//! Qualified analysis upload, isolated from plan and task evidence.

use crate::{RenderError, steps};
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepKind};

/// Producer outputs; paths and names come only from successful fresh staging.
pub const ARTIFACT_NAME_OUTPUT: &str = "analysis_artifact_name";
/// The producer's isolated single-file upload path.
pub const ARTIFACT_PATH_OUTPUT: &str = "analysis_artifact_path";
/// Analysis upload display name.
pub const UPLOAD_NAME: &str = "Publish Cargo analysis";

/// Only the producer's exact keyed output pair can enter action inputs.
pub(crate) fn is_output_binding(key: &str, value: &str) -> bool {
    match key {
        "name" => value == format!("${{{{ steps.plan.outputs.{ARTIFACT_NAME_OUTPUT} }}}}"),
        "path" => value == format!("${{{{ steps.plan.outputs.{ARTIFACT_PATH_OUTPUT} }}}}"),
        _ => false,
    }
}

/// Bind admitted output inputs to the complete pinned publication factory.
/// # Errors
/// Rejects foreign jobs, actions, conditions, and modified output bindings.
pub(crate) fn validate_upload_binding(job_id: &str, step: &Step) -> Result<(), RenderError> {
    let StepKind::Action { with, .. } = &step.kind else {
        return Ok(());
    };
    let references_analysis = with
        .values()
        .any(|value| value.contains(ARTIFACT_NAME_OUTPUT) || value.contains(ARTIFACT_PATH_OUTPUT));
    if !references_analysis {
        return Ok(());
    }
    if job_id == crate::PLAN_JOB_ID && *step == upload_step()? {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(
            "analysis_upload_without_factory".to_owned(),
        ))
    }
}

/// Upload only protected default pushes. Retrieval additionally verifies the
/// complete workflow run concluded successfully before authorizing reuse.
/// # Errors
/// Returns render errors when the fixed action shape fails validation.
pub fn upload_step() -> Result<Step, RenderError> {
    let mut step = steps::action_step(
        UPLOAD_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "name".to_owned(),
                format!("${{{{ steps.plan.outputs.{ARTIFACT_NAME_OUTPUT} }}}}"),
            ),
            (
                "path".to_owned(),
                format!("${{{{ steps.plan.outputs.{ARTIFACT_PATH_OUTPUT} }}}}"),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "90".to_owned()),
        ]),
    )?;
    step.condition = Some(format!(
        "success() && github.event_name == 'push' && github.ref == format('refs/heads/{{0}}', github.event.repository.default_branch) && github.ref_protected == true && steps.plan.outputs.{ARTIFACT_NAME_OUTPUT} != ''"
    ));
    Ok(step)
}

#[cfg(test)]
#[path = "analysis_publication_tests.rs"]
mod tests;

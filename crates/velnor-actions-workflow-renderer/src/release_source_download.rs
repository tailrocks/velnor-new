//! Exact private artifact action boundary for a credential-free source consumer.
use crate::{RenderError, steps::DOWNLOAD_ARTIFACT_USES};
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepKind};

pub(super) fn check(step: &Step, index: usize) -> Result<(), RenderError> {
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(invalid());
    };
    let expected = BTreeMap::from([
        (
            "artifact-ids".to_owned(),
            "${{ needs.release-source-snapshot.outputs.source-snapshot-artifact-id }}".to_owned(),
        ),
        (
            "path".to_owned(),
            "${{ runner.temp }}/velnor/release-source-input".to_owned(),
        ),
    ]);
    if index != 2
        || uses != DOWNLOAD_ARTIFACT_USES
        || with != &expected
        || !env.is_empty()
        || step.id.is_some()
        || step.condition.is_some()
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> RenderError {
    RenderError::InvalidWorkflow("release_source_download_authority".to_owned())
}

//! Standalone prepared-package leaf fixture.
use std::collections::BTreeMap;

use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation as Op, Step, StepId};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;

use super::helper_record;

pub(crate) fn helper_registry(bootstrap_mode: bool) -> Vec<CompiledSourceHelper> {
    let mut records = super::super::bootstrap_fixture::prepared_tool_records();
    records.push(helper_record(
        ReleaseRole::PackagePreparedAnonymous,
        Op::RustReleasePreparedPackage,
        bootstrap_mode,
    ));
    records
}

pub(crate) fn steps(bootstrap_mode: bool) -> Result<Vec<Step>, RenderError> {
    let mut steps = Vec::new();
    for record in super::super::bootstrap_fixture::prepared_tool_records() {
        steps.push(
            velnor_actions_workflow_renderer::source_helper::source_helper_step(
                "Prepare qualified anonymous tools",
                &record,
                record.environment().clone(),
            )?,
        );
    }
    steps.push(velnor_actions_workflow_renderer::action_step(
        "Download source snapshot",
        velnor_actions_workflow_renderer::steps::DOWNLOAD_ARTIFACT_USES,
        BTreeMap::from([
            (
                "artifact-ids".to_owned(),
                "${{ needs.release-source-snapshot.outputs.source-snapshot-artifact-id }}"
                    .to_owned(),
            ),
            (
                "path".to_owned(),
                "${{ runner.temp }}/velnor/release-source-input".to_owned(),
            ),
        ]),
    )?);
    let role = ReleaseRole::PackagePreparedAnonymous;
    let mut prepared = super::helper_step(
        role,
        Op::RustReleasePreparedPackage,
        bootstrap_mode,
        "Prepare anonymous package",
    )?;
    prepared.id = Some(StepId::new("release-package").map_err(RenderError::Contract)?);
    steps.push(prepared);
    steps.push(super::upload(role)?);
    Ok(steps)
}

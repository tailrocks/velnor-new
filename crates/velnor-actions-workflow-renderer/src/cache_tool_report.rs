//! Closed terminal reporter source and exact evidence controls.
use super::invalid;
use crate::RenderError;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, PureToolProducer, SourceBoundHelper,
    SourceBoundOperation, Step, StepKind,
};

fn report_environment(meta: &PureToolProducer) -> BTreeMap<String, String> {
    let output = |id: &velnor_actions_contract::StepId, key: &str| {
        format!("${{{{ steps.{}.outputs.{key} }}}}", id.as_str())
    };
    let outcome =
        |id: &velnor_actions_contract::StepId| format!("${{{{ steps.{}.outcome }}}}", id.as_str());
    let mut env = BTreeMap::from([
        (
            "VELNOR_TOOL_DESCRIPTOR_IDENTITY".to_owned(),
            meta.descriptor.immutable_identity.clone(),
        ),
        (
            "VELNOR_TOOL_IDENTITY".to_owned(),
            format!(
                "{}-${{{{ env.VELNOR_CACHE_IMAGE }}}}",
                meta.descriptor.immutable_identity
            ),
        ),
        (
            "VELNOR_TOOL_VERIFIED".to_owned(),
            output(&meta.installation_step, "verified"),
        ),
        (
            "VELNOR_TOOL_BEFORE_AVAILABLE".to_owned(),
            output(&meta.before_step, "available"),
        ),
        (
            "VELNOR_TOOL_AFTER_AVAILABLE".to_owned(),
            output(&meta.after_step, "available"),
        ),
        (
            "VELNOR_TOOL_BEFORE_DIGEST".to_owned(),
            output(&meta.before_step, "digest"),
        ),
        (
            "VELNOR_TOOL_AFTER_DIGEST".to_owned(),
            output(&meta.after_step, "digest"),
        ),
        (
            "VELNOR_TOOL_BEFORE_OUTCOME".to_owned(),
            outcome(&meta.before_step),
        ),
        (
            "VELNOR_TOOL_AFTER_OUTCOME".to_owned(),
            outcome(&meta.after_step),
        ),
        (
            "VELNOR_TOOL_CHANGED".to_owned(),
            output(&meta.after_step, "changed"),
        ),
        (
            "VELNOR_TOOL_MATCHED_KEY".to_owned(),
            output(&meta.restore_step, "cache-matched-key"),
        ),
        (
            "VELNOR_TOOL_INSTALL_OUTCOME".to_owned(),
            outcome(&meta.installation_step),
        ),
        (
            "VELNOR_TOOL_RESTORE_OUTCOME".to_owned(),
            outcome(&meta.restore_step),
        ),
        (
            "VELNOR_TOOL_SAVE_OUTCOME".to_owned(),
            outcome(&meta.save_step),
        ),
    ]);
    env.extend(publication_environment(meta));
    env
}

fn publication_environment(meta: &PureToolProducer) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "VELNOR_TOOL_PUBLICATION_EXACT_HIT".to_owned(),
            "${{ steps.velnor-tool-publication.outputs.cache-hit }}".to_owned(),
        ),
        (
            "VELNOR_TOOL_PUBLICATION_OUTCOME".to_owned(),
            "${{ steps.velnor-tool-publication.outcome }}".to_owned(),
        ),
        (
            "VELNOR_TOOL_PUBLICATION_MATCHED_KEY".to_owned(),
            "${{ steps.velnor-tool-publication.outputs.cache-matched-key }}".to_owned(),
        ),
        (
            "VELNOR_TOOL_PUBLICATION_EXPECTED_KEY".to_owned(),
            format!(
                "{}-${{{{ env.VELNOR_CACHE_IMAGE }}}}-snapshot-${{{{ steps.{}.outputs.digest }}}}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}",
                meta.descriptor.immutable_identity,
                meta.after_step.as_str()
            ),
        ),
    ])
}

/// Exact compiled terminal reporter; no repository executable is involved.
/// # Errors
/// Rejects malformed version markers or compiled helper descriptors.
pub fn report_record(
    meta: &PureToolProducer,
    version: &str,
) -> Result<CompiledSourceHelper, RenderError> {
    let python = include_str!("../tools/tool_producer_report.py");
    let body = format!(
        "set -eu\n/usr/bin/python3 -I -S - <<'VELNOR_TOOL_REPORT'\n{python}\nVELNOR_TOOL_REPORT\n"
    );
    let source = crate::marker::with_marker(version, &body)?;
    let operation = SourceBoundOperation::ToolProducerReport;
    let descriptor = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
    )
    .map_err(RenderError::Contract)?;
    let invocation = HelperInvocation::compiled(descriptor, Vec::new(), Vec::new())
        .map_err(RenderError::Contract)?;
    CompiledSourceHelper::compiled(invocation, source)
        .map(|record| record.with_environment(report_environment(meta)))
        .map_err(RenderError::Contract)
}

pub(super) fn report_step(meta: &PureToolProducer, version: &str) -> Result<Step, RenderError> {
    let record = report_record(meta, version)?;
    let mut step = crate::source_helper::source_helper_step(
        "Report executable cache availability",
        &record,
        record.environment().clone(),
    )?;
    step.id = Some(meta.report_step.clone());
    step.condition = Some("always()".to_owned());
    Ok(step)
}

pub(super) fn validate_report(step: &Step, meta: &PureToolProducer) -> Result<(), RenderError> {
    if step.id.as_ref() != Some(&meta.report_step)
        || step.name != "Report executable cache availability"
        || step.condition.as_deref() != Some("always()")
        || !matches!(&step.kind, StepKind::SourceBoundHelper { invocation, env }
            if invocation.descriptor().operation() == SourceBoundOperation::ToolProducerReport
            && invocation.args().is_empty() && invocation.installed_selectors().is_empty()
            && *env == report_environment(meta))
    {
        return Err(invalid("tool_producer_report_changed"));
    }
    Ok(())
}

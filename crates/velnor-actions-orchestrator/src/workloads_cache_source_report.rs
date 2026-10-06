//! Mandatory terminal source qualification, independent of producer conclusion.
use crate::OrchestratorError;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    SourceProducer, Step,
};

/// Stable terminal evidence identifier used by source producer owners.
pub(crate) const REPORT_ID: &str = "velnor-source-report";

/// Wait for optional source production without suppressing ordinary installation.
pub(crate) fn depend_on_producer(consumer: &mut velnor_actions_contract::Job, producer_id: &str) {
    if consumer.needs.iter().any(|need| need == producer_id) {
        return;
    }
    let prior_success = if consumer.needs.is_empty() {
        "true".to_owned()
    } else {
        consumer
            .needs
            .iter()
            .map(|need| format!("needs['{need}'].result == 'success'"))
            .collect::<Vec<_>>()
            .join(" && ")
    };
    let original = consumer
        .condition
        .take()
        .unwrap_or_else(|| "success()".to_owned());
    let original = original
        .strip_prefix("${{")
        .and_then(|s| s.strip_suffix("}}"))
        .map_or(original.as_str(), str::trim);
    let original = original.replace("success()", &format!("({prior_success})"));
    consumer.needs.push(producer_id.to_owned());
    consumer.condition = Some(format!(
        "!cancelled() && needs.plan.result == 'success' && ({original})"
    ));
}

/// Look up the exact publication key without downloading its payload.
pub(crate) fn publication_step(
    meta: &SourceProducer,
    paths: &[String],
) -> Result<Step, OrchestratorError> {
    meta.validate()?;
    let mut step = velnor_actions_workflow_renderer::action_step(
        "Verify source cache publication",
        velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES,
        BTreeMap::from([
            ("key".to_owned(), meta.save_key()),
            ("lookup-only".to_owned(), "true".to_owned()),
            ("path".to_owned(), paths.join("\n")),
        ]),
    )?;
    step.id = Some(meta.publication_step.clone());
    step.condition = Some(meta.publication_condition());
    Ok(step)
}

/// Compile a terminal observation bound to literal source authority.
pub(crate) fn record(
    meta: &SourceProducer,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    meta.validate()?;
    let source = velnor_actions_contract::generated_source(
        version,
        include_str!("workloads_cache_source_report.sh"),
    )?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::SourceProducerReport;
    let helper = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let invocation = HelperInvocation::compiled(helper, Vec::new(), Vec::new())?;
    Ok(CompiledSourceHelper::compiled(invocation, source)?.with_environment(environment(meta)))
}

/// Always emit the terminal report, including failed bootstrap or transport.
pub(crate) fn step(meta: &SourceProducer, version: &str) -> Result<Step, OrchestratorError> {
    let record = record(meta, version)?;
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Report source availability",
        &record,
        record.environment().clone(),
    )?;
    step.id = Some(meta.report_step.clone());
    step.condition = Some("always()".to_owned());
    Ok(step)
}

/// Export the complete terminal source report through the qualified report step.
#[must_use]
pub(crate) fn outputs(meta: &SourceProducer) -> Vec<velnor_actions_contract::workflow::JobOutput> {
    use velnor_actions_contract::workflow::{ActionOutput, JobOutput, StepOutputRef};
    [
        ActionOutput::CacheAvailable,
        ActionOutput::Verified,
        ActionOutput::SourceIdentity,
        ActionOutput::Error,
    ]
    .into_iter()
    .map(|output| JobOutput {
        name: output.as_str().to_owned(),
        value: StepOutputRef {
            step_id: meta.report_step.clone(),
            output,
        },
    })
    .collect()
}

fn environment(meta: &SourceProducer) -> BTreeMap<String, String> {
    let proof = meta.verification_step.as_str();
    let save = meta.save_step.as_str();
    let restore = meta.restore_step.as_str();
    let publication = meta.publication_step.as_str();
    let (snapshot_outcome, snapshot_changed) = match meta.role {
        velnor_actions_contract::SourceProducerRole::Cargo => (
            "${{ steps.velnor-rust-source-after.outcome }}",
            "${{ env.VELNOR_SOURCES_SNAPSHOT_CHANGED }}",
        ),
        velnor_actions_contract::SourceProducerRole::Npm => (
            "${{ steps.velnor-npm-source-after.outcome }}",
            "${{ env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_CHANGED }}",
        ),
        velnor_actions_contract::SourceProducerRole::Bun => (
            "${{ steps.velnor-bun-source-after.outcome }}",
            "${{ env.VELNOR_BUN_DOWNLOADS_SNAPSHOT_CHANGED }}",
        ),
        _ => ("success", "true"),
    };
    BTreeMap::from([
        (
            "VELNOR_SOURCE_SNAPSHOT_OUTCOME".to_owned(),
            snapshot_outcome.to_owned(),
        ),
        (
            "VELNOR_SOURCE_SNAPSHOT_CHANGED".to_owned(),
            snapshot_changed.to_owned(),
        ),
        (
            "VELNOR_SOURCE_IDENTITY".to_owned(),
            meta.source_identity.clone(),
        ),
        (
            "VELNOR_SOURCE_OUTCOME".to_owned(),
            format!("${{{{ steps.{proof}.outcome }}}}"),
        ),
        (
            "VELNOR_SOURCE_VERIFIED".to_owned(),
            format!("${{{{ steps.{proof}.outputs.verified }}}}"),
        ),
        (
            "VELNOR_SOURCE_ERROR".to_owned(),
            format!("${{{{ steps.{proof}.outputs.error }}}}"),
        ),
        (
            "VELNOR_SOURCE_RESTORE_OUTCOME".to_owned(),
            format!("${{{{ steps.{restore}.outcome }}}}"),
        ),
        (
            "VELNOR_SOURCE_RESTORE_KEY".to_owned(),
            format!("${{{{ steps.{restore}.outputs.cache-matched-key }}}}"),
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_OUTCOME".to_owned(),
            format!("${{{{ steps.{publication}.outcome }}}}"),
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_MATCHED_KEY".to_owned(),
            format!("${{{{ steps.{publication}.outputs.cache-matched-key }}}}"),
        ),
        (
            "VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY".to_owned(),
            meta.save_key(),
        ),
        (
            "VELNOR_SOURCE_SAVE_OUTCOME".to_owned(),
            format!("${{{{ steps.{save}.outcome }}}}"),
        ),
    ])
}

#[cfg(test)]
#[path = "workloads_cache_source_report_tests.rs"]
mod tests;

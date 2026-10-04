//! Compile the universal Mise-owned cache observer for closed payload domains.

use velnor_actions_contract::{
    CacheSnapshotDomain, CompiledSourceHelper, HelperInvocation, SourceBoundHelper,
    SourceBoundOperation, Step,
};

use crate::OrchestratorError;

/// Payload admission belongs to the enclosing role; this factory accepts closed domains.
pub(crate) fn record(
    domain: CacheSnapshotDomain,
    before: bool,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let body = velnor_actions_mise::cache_snapshot::snapshot_source()?;
    let source = velnor_actions_contract::generated_source(version, &body)?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::CacheSnapshot;
    let helper = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let phase = if before { "before" } else { "after" };
    let invocation = HelperInvocation::compiled(
        helper,
        vec![domain.name().to_owned(), phase.to_owned()],
        Vec::new(),
    )?;
    let environment = domain.environment(before);
    Ok(CompiledSourceHelper::compiled(invocation, source)?.with_environment(environment))
}

/// Bind a named observation to its exact compiled source and environment.
pub(crate) fn step(
    domain: CacheSnapshotDomain,
    before: bool,
    version: &str,
) -> Result<Step, OrchestratorError> {
    let record = record(domain, before, version)?;
    let phase = if before { "before" } else { "after" };
    Ok(
        velnor_actions_workflow_renderer::source_helper::source_helper_step(
            &format!("Measure {} snapshot {phase}", domain.name()),
            &record,
            record.environment().clone(),
        )?,
    )
}

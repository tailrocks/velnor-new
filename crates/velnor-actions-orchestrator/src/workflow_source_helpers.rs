//! Reconstruct tool preparation authority from final IR through compiled owners.

use velnor_actions_contract::{
    CompiledSourceHelper, Job, SourceBoundOperation, StepKind, WorkflowIr,
};

use crate::OrchestratorError;

/// Qualify every tool preparation against its catalog factory and exact environment.
pub(crate) fn collect_rust_source_helpers(
    ir: &WorkflowIr,
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    collect_rust_job_helpers(ir.jobs.values(), version)
}

/// Qualify final normalized jobs before source draft signing.
pub(crate) fn collect_rust_job_helpers<'a>(
    jobs: impl IntoIterator<Item = &'a Job>,
    version: &str,
) -> Result<Vec<CompiledSourceHelper>, OrchestratorError> {
    let mut records = Vec::new();
    for job in jobs {
        for step in &job.steps {
            let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
                continue;
            };
            if !matches!(
                invocation.descriptor().operation(),
                SourceBoundOperation::MiseToolPrepare
                    | SourceBoundOperation::RustPrepareRootLinux
                    | SourceBoundOperation::RustPrepareDesktopMac
                    | SourceBoundOperation::RustPrepareDesktopSourceMac
            ) {
                continue;
            }
            let record = velnor_actions_mise::catalog::tool_prepare::record_for_invocation(
                invocation, env, version,
            )
            .map_err(|error| OrchestratorError::Contract {
                problem: error.to_string(),
            })?;
            if !records.contains(&record) {
                records.push(record);
            }
        }
    }
    Ok(records)
}

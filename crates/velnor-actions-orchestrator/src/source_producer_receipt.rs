//! Reconstitute a Rust source record through its sole compiled owner.

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundOperation, SourceProducer,
    SourceProducerRole, ToolCacheDomain,
};

use super::{
    descriptor::{RustSourceDescriptor, RustSourceProjection},
    transport::Source3,
};
use crate::OrchestratorError;

/// Describe source compatibility only after the sole compiled owner admits it.
pub(crate) fn source_compatibility_projection(
    invocation: &HelperInvocation,
    metadata: &SourceProducer,
    version: &str,
) -> Result<RustSourceProjection, OrchestratorError> {
    let record = record_for_receipt(invocation, metadata, version)?;
    let [encoded] = record.invocation().args() else {
        return Err(reject());
    };
    RustSourceDescriptor::from_hex(encoded)?.source_projection()
}

pub(crate) fn record_for_receipt(
    invocation: &HelperInvocation,
    metadata: &SourceProducer,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    metadata.validate()?;
    let tool = metadata.tool_cache.as_ref().ok_or_else(reject)?;
    if metadata.role != SourceProducerRole::Cargo
        || tool.domain != ToolCacheDomain::Full
        || tool.target != "x86_64-unknown-linux-gnu"
        || invocation.descriptor().operation() != SourceBoundOperation::RustSourceProducer
        || !invocation.installed_selectors().is_empty()
    {
        return Err(reject());
    }
    let [encoded] = invocation.args() else {
        return Err(reject());
    };
    let descriptor = RustSourceDescriptor::from_hex(encoded)?;
    let record = super::source::compiled_helper(&descriptor, version)?;
    let identity = super::descriptor::source_identity(&descriptor, &record)?;
    if identity != metadata.source_identity || record.invocation() != invocation {
        return Err(reject());
    }
    let source = Source3::new(identity)?;
    Ok(record.with_environment(super::job::helper_environment(&source)))
}

fn reject() -> OrchestratorError {
    OrchestratorError::Contract {
        problem: "rust_source_receipt_owner_mismatch".to_owned(),
    }
}

#[cfg(test)]
#[path = "source_producer_receipt_tests.rs"]
mod tests;

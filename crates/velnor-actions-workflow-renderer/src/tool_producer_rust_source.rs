//! Exact compiled Rust preparation admitted inside the Cargo source producer.

use crate::{MiseSetup, RenderError};
use velnor_actions_contract::{
    CompiledSourceHelper, SourceBoundOperation, Step, StepKind, ToolCacheDescriptor,
    ToolCacheDomain, tool_target_for_runner_label,
};

/// Admit only the canonical full-domain root Linux compiler preparation.
/// # Errors
/// Rejects foreign source scope, altered compiled authority, or cache identity.
pub fn is_rust_source_preparation(
    descriptor: &ToolCacheDescriptor,
    step: &Step,
    setup: &MiseSetup,
    records: &[CompiledSourceHelper],
) -> Result<bool, RenderError> {
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        return Ok(false);
    };
    if invocation.descriptor().operation() != SourceBoundOperation::RustPrepareRootLinux {
        return Ok(false);
    }
    if descriptor.domain != ToolCacheDomain::Full
        || env.get("RUSTUP_AUTO_INSTALL").map(String::as_str) != Some("0")
        || descriptor.target != "x86_64-unknown-linux-gnu"
        || tool_target_for_runner_label(&descriptor.runs_on) != Some(descriptor.target.as_str())
        || !matches!(descriptor.selectors.as_slice(), [selector]
            if selector.starts_with("rust[profile=minimal,components=clippy,rustfmt]@"))
        || invocation.installed_selectors() != descriptor.selectors
        || step.condition.is_some()
        || step
            .id
            .as_ref()
            .map(velnor_actions_contract::StepId::as_str)
            != Some("velnor-rust-source-prepare")
        || invocation.args().first().map(String::as_str) != Some("tools")
    {
        return Err(RenderError::InvalidWorkflow(
            "rust_source_prepare_foreign_scope".to_owned(),
        ));
    }
    if !records
        .iter()
        .any(|record| record.invocation() == invocation && record.environment() == env)
    {
        return Err(RenderError::InvalidWorkflow(
            "rust_source_prepare_authority_missing".to_owned(),
        ));
    }
    super::validate_tool_descriptor(descriptor, setup, step, records)?;
    Ok(true)
}

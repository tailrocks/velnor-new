//! Anonymous Python control, separate from cold compiler and GitHub roles.

use crate::catalog::native_tool_context::qualified_native_execution_environment;
use crate::catalog::qualification::DistributionHost;
use crate::catalog::{mise_acquisition, tool_prepare};
use crate::{MiseError, PinnedTool, ToolCatalog};
use velnor_actions_contract::workflow::native_tools::{
    CompiledNativeExecRecipe, NativeCredentialScope,
};
use velnor_actions_contract::{CompiledSourceHelper, ToolCacheDomain};

/// Closed preparation and execution records for anonymous source control.
///
/// This generation-time record grants no cold compiler or native verifier
/// capability. Actual runtime installation authority remains independent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceIntentControlTools {
    bootstrap: CompiledSourceHelper,
    preparation: CompiledSourceHelper,
    execution: CompiledNativeExecRecipe,
}

/// Resolve Python alone for the actual anonymous control host.
/// # Errors
/// Rejects missing owned Mise or Python installation qualification.
pub fn source_intent_control_tools(
    host: DistributionHost,
    generator_version: &str,
) -> Result<SourceIntentControlTools, MiseError> {
    let catalog = ToolCatalog::pinned();
    let tools = [PinnedTool::Python];
    let mut environment =
        qualified_native_execution_environment(&catalog, host, ToolCacheDomain::Full, &tools)?;
    environment.extend([
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        ("PYTHONNOUSERSITE".to_owned(), "1".to_owned()),
        (
            "HOME".to_owned(),
            "${{ runner.temp }}/velnor/source-intent-control-home".to_owned(),
        ),
        (
            "MISE_CONFIG_DIR".to_owned(),
            "${{ runner.temp }}/velnor/mise-config".to_owned(),
        ),
        (
            "MISE_CACHE_DIR".to_owned(),
            "${{ runner.temp }}/velnor/mise-cache".to_owned(),
        ),
        (
            "MISE_STATE_DIR".to_owned(),
            "${{ runner.temp }}/velnor/mise-state".to_owned(),
        ),
    ]);
    let selectors = catalog.native_tool_specs(host, &tools)?;
    let bootstrap =
        mise_acquisition::helper_for_domain(ToolCacheDomain::Full, host, generator_version)?;
    let preparation = tool_prepare::helper_for_tools(
        &catalog,
        ToolCacheDomain::Full,
        host,
        &selectors,
        generator_version,
    )?;
    let execution = super::recipe_from_authority(
        &catalog,
        host,
        &tools,
        environment,
        NativeCredentialScope::Anonymous,
    )?;
    Ok(SourceIntentControlTools {
        bootstrap,
        preparation,
        execution,
    })
}

/// Reconstruct every host, source, selector, scope and environment binding.
/// # Errors
/// Rejects changed records or unavailable qualification.
pub fn validate_source_intent_control_tools(
    host: DistributionHost,
    generator_version: &str,
    record: &SourceIntentControlTools,
) -> Result<(), MiseError> {
    if &source_intent_control_tools(host, generator_version)? != record {
        return Err(MiseError::Contract {
            problem: "source_intent_control_authority_changed".to_owned(),
        });
    }
    Ok(())
}

impl SourceIntentControlTools {
    /// Full-domain owned Mise acquisition record.
    #[must_use]
    pub const fn bootstrap(&self) -> &CompiledSourceHelper {
        &self.bootstrap
    }

    /// Python-only Full-domain preparation record.
    #[must_use]
    pub const fn preparation(&self) -> &CompiledSourceHelper {
        &self.preparation
    }

    /// Anonymous Python-only envelope; Full home bindings select no compiler or GitHub tool.
    #[must_use]
    pub const fn execution(&self) -> &CompiledNativeExecRecipe {
        &self.execution
    }
}

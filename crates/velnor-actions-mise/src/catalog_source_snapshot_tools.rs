//! Closed source-snapshot tools with separate Full and Planning Mise owners.

use velnor_actions_contract::workflow::native_tools::{
    CompiledNativeExecRecipe, NativeCredentialScope,
};
use velnor_actions_contract::{CompiledSourceHelper, ToolCacheDomain};

use crate::catalog::native_tool_context::snapshot_control::{
    QualifiedSourceSnapshotContext, qualified_source_snapshot_context,
};
use crate::catalog::qualification::DistributionHost;
use crate::catalog::{mise_acquisition, tool_prepare};
use crate::{MiseError, PinnedTool, ToolCatalog};

/// Environment key used by the compiled source owner to invoke Planning `gh`.
pub const SOURCE_SNAPSHOT_GH_ENV: &str = "VELNOR_SOURCE_SNAPSHOT_GH";

/// Complete SDK-owned source-snapshot tool closure.
///
/// The record is generation-time authority only. It deliberately has no
/// serialization implementation; callers must reconstruct it from the pinned
/// catalog and the explicit host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSnapshotTools {
    context: QualifiedSourceSnapshotContext,
    execution: CompiledNativeExecRecipe,
    bootstrap_records: [CompiledSourceHelper; 2],
    preparation_records: [CompiledSourceHelper; 2],
}

/// Build the complete source-snapshot tool closure for one host.
///
/// Full owns Python only. Planning owns GitHub only. The two bootstrap and
/// preparation records stay separate so a combined Full/Python/GitHub selector
/// set cannot acquire or execute either tool.
///
/// # Errors
/// Propagates typed qualification failures for missing owned Mise or host
/// distributions, plus malformed SDK recipes.
pub fn source_snapshot_tools(
    host: DistributionHost,
    generator_version: &str,
) -> Result<SourceSnapshotTools, MiseError> {
    let catalog = ToolCatalog::pinned();
    let context = qualified_source_snapshot_context(&catalog, host)?;
    let full_selectors = context.full_selectors();
    let planning_selectors = vec![context.github().selector().to_owned()];
    let bootstrap_records = [
        mise_acquisition::helper_for_domain(ToolCacheDomain::Full, host, generator_version)?,
        mise_acquisition::helper_for_domain(ToolCacheDomain::Planning, host, generator_version)?,
    ];
    let preparation_records = [
        tool_prepare::helper_for_tools(
            &catalog,
            ToolCacheDomain::Full,
            host,
            &full_selectors,
            generator_version,
        )?,
        tool_prepare::helper_for_tools(
            &catalog,
            ToolCacheDomain::Planning,
            host,
            &planning_selectors,
            generator_version,
        )?,
    ];
    let execution = execution_recipe(&catalog, host, &context)?;
    Ok(SourceSnapshotTools {
        context,
        execution,
        bootstrap_records,
        preparation_records,
    })
}

/// Reconstruct the full source-snapshot closure and compare every authority
/// record exactly.
///
/// # Errors
/// Rejects any changed context, source helper, execution recipe, host, or
/// generator-version-bound source.
pub fn validate_source_snapshot_tools(
    host: DistributionHost,
    generator_version: &str,
    record: &SourceSnapshotTools,
) -> Result<(), MiseError> {
    if &source_snapshot_tools(host, generator_version)? != record {
        return Err(invalid("source_snapshot_tools_authority_changed"));
    }
    Ok(())
}

impl SourceSnapshotTools {
    /// Sealed Python Full and GitHub Planning qualification context.
    #[must_use]
    pub const fn context(&self) -> &QualifiedSourceSnapshotContext {
        &self.context
    }

    /// Python-only Full execution recipe with read-only GitHub credentials.
    #[must_use]
    pub const fn execution(&self) -> &CompiledNativeExecRecipe {
        &self.execution
    }

    /// Independent Full and Planning Mise acquisition records, in that order.
    #[must_use]
    pub const fn bootstrap_records(&self) -> &[CompiledSourceHelper; 2] {
        &self.bootstrap_records
    }

    /// Independent Full/Python and Planning/GitHub preparation records.
    #[must_use]
    pub const fn preparation_records(&self) -> &[CompiledSourceHelper; 2] {
        &self.preparation_records
    }
}

fn execution_recipe(
    catalog: &ToolCatalog,
    host: DistributionHost,
    context: &QualifiedSourceSnapshotContext,
) -> Result<CompiledNativeExecRecipe, MiseError> {
    let selectors = context.full_selectors();
    let catalog_selectors = catalog.native_tool_specs(host, &[PinnedTool::Python])?;
    if selectors != catalog_selectors {
        return Err(invalid("source_snapshot_python_selector_changed"));
    }
    let mut environment = context.environment().clone();
    environment.extend([
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
        (
            "HOME".to_owned(),
            "${{ runner.temp }}/velnor/delivery-home".to_owned(),
        ),
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        ("PYTHONNOUSERSITE".to_owned(), "1".to_owned()),
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        (
            SOURCE_SNAPSHOT_GH_ENV.to_owned(),
            context.github().executable(),
        ),
    ]);
    let mut prefix = vec!["/usr/bin/env".to_owned(), "-i".to_owned()];
    prefix.extend(environment.iter().map(|(key, value)| {
        let value = if NativeCredentialScope::GithubReadOnly
            .allowed_keys()
            .contains(&key.as_str())
        {
            format!("${key}")
        } else {
            value.replace("${{ runner.temp }}", "$RUNNER_TEMP")
        };
        format!("{key}={value}")
    }));
    prefix.extend(super::managed_exec_prefix(
        catalog,
        host,
        &[PinnedTool::Python],
    )?);
    CompiledNativeExecRecipe::compiled_for_scope(
        prefix,
        environment,
        selectors,
        NativeCredentialScope::GithubReadOnly,
    )
    .map_err(|error| invalid(&error.to_string()))
}

fn invalid(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_snapshot_tools_fail_closed_without_owned_host_publication() {
        for host in [
            DistributionHost::LinuxAmd64,
            DistributionHost::LinuxArm64,
            DistributionHost::MacosArm64,
        ] {
            assert!(source_snapshot_tools(host, "0.1.0").is_err());
        }
    }
}

//! Closed two-domain control for read-only release admission.
//!
//! Admission owns Python from Full and GitHub from Planning. Its execution
//! recipe launches Python only; the qualified Planning GitHub path is passed
//! through a dedicated environment binding.

use super::managed_exec_prefix;
use crate::catalog::native_tool_context::admission_control::{
    QualifiedAdmissionContext, qualified_admission_context,
};
use crate::catalog::qualification::DistributionHost;
use crate::catalog::{mise_acquisition, tool_prepare};
use crate::{MiseError, PinnedTool, ToolCatalog};
use velnor_actions_contract::CompiledSourceHelper;
use velnor_actions_contract::ToolCacheDomain;
use velnor_actions_contract::workflow::native_tools::{
    CompiledNativeExecRecipe, NativeCredentialScope,
};

/// Environment key carrying the immutable qualified Planning-domain `gh`.
pub const ADMISSION_PLANNING_GH_ENV: &str = "VELNOR_ADMISSION_PLANNING_GH";

/// Complete SDK-owned read-only admission tool closure.
///
/// The record is generation-time authority only. Its fields stay private so
/// callers must reconstruct the complete catalog record through
/// [`admission_tools`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionTools {
    context: QualifiedAdmissionContext,
    execution: CompiledNativeExecRecipe,
    bootstrap_records: [CompiledSourceHelper; 2],
    preparation_records: [CompiledSourceHelper; 2],
}

/// Build the complete read-only admission tool closure for one host.
///
/// Full owns Python only. Planning owns GitHub only. Bootstrap and
/// preparation records remain separate, so the Python execution envelope
/// cannot silently acquire a Full-domain GitHub selector.
/// # Errors
/// Propagates typed failures for missing owned publication, host qualification,
/// or altered native recipe construction.
pub fn admission_tools(
    host: DistributionHost,
    generator_version: &str,
) -> Result<AdmissionTools, MiseError> {
    let catalog = ToolCatalog::pinned();
    let context = qualified_admission_context(&catalog, host)?;
    let full_selectors = context.full_selectors();
    let planning_selectors = vec![context.gh_planning().selector().to_owned()];
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
    let execution = execution_recipe(&catalog, host, &context, full_selectors)?;
    Ok(AdmissionTools {
        context,
        execution,
        bootstrap_records,
        preparation_records,
    })
}

/// Reconstruct the complete admission closure and compare every authority.
/// # Errors
/// Rejects changed context, source helpers, execution recipe, host, or marker.
pub fn validate_admission_tools(
    host: DistributionHost,
    generator_version: &str,
    record: &AdmissionTools,
) -> Result<(), MiseError> {
    if &admission_tools(host, generator_version)? != record {
        return Err(invalid("admission_tools_authority_changed"));
    }
    Ok(())
}

impl AdmissionTools {
    /// Sealed Python Full/GitHub Planning admission context.
    #[must_use]
    pub const fn context(&self) -> &QualifiedAdmissionContext {
        &self.context
    }

    /// Python-only Full-domain execution recipe.
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
    context: &QualifiedAdmissionContext,
    selectors: Vec<String>,
) -> Result<CompiledNativeExecRecipe, MiseError> {
    let catalog_selectors = catalog.native_tool_specs(host, &[PinnedTool::Python])?;
    if selectors != catalog_selectors {
        return Err(invalid("admission_python_selector_changed"));
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
            "${{ runner.temp }}/velnor/admission-home".to_owned(),
        ),
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        ("PYTHONNOUSERSITE".to_owned(), "1".to_owned()),
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        (
            ADMISSION_PLANNING_GH_ENV.to_owned(),
            context.gh_planning().executable(),
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
    prefix.extend(managed_exec_prefix(catalog, host, &[PinnedTool::Python])?);
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
    fn admission_uses_a_dedicated_planning_github_binding() {
        assert_eq!(ADMISSION_PLANNING_GH_ENV, "VELNOR_ADMISSION_PLANNING_GH");
    }

    #[test]
    fn admission_fails_closed_without_owned_publication() {
        let catalog = ToolCatalog::pinned();
        for host in [
            DistributionHost::LinuxAmd64,
            DistributionHost::LinuxArm64,
            DistributionHost::MacosArm64,
        ] {
            assert!(qualified_admission_context(&catalog, host).is_err());
            assert!(admission_tools(host, "0.1.0").is_err());
        }
    }
}

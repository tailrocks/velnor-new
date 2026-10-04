//! Qualified compiler execution environment from compiled distribution authority.

use std::collections::BTreeMap;

use super::{
    DistributionHost, DistributionRequirement, DistributionTool, MiseError, PinnedTool,
    QualifiedDistribution, ToolCatalog, distribution_host, invalid, role_for_catalog,
};

/// Bind the complete selected tool identity and measured native launch homes.
/// # Errors
/// Rejects mismatched hosts or absent launch/distribution qualification.
pub fn qualified_exec_environment_for_tools(
    catalog: &ToolCatalog,
    host: DistributionHost,
    tools: &[PinnedTool],
) -> Result<BTreeMap<String, String>, MiseError> {
    if host != distribution_host(role_for_catalog(catalog)?) {
        return Err(invalid());
    }
    if tools.iter().any(|tool| {
        matches!(tool, PinnedTool::Rust | PinnedTool::RustDesktop)
            && *tool != catalog.compiler_tool()
    }) {
        return Err(invalid());
    }
    let mise = QualifiedDistribution::require_for_generator(
        DistributionTool::Mise,
        host,
        DistributionRequirement::RequiresNoMiserc,
    )?;
    let mut qualifications = vec![mise.clone()];
    let mut environment = BTreeMap::new();
    for (key, value) in crate::ToolHomes::runner_temp().exec_env(catalog) {
        environment.insert(
            key.into_string().map_err(|_| invalid())?,
            value.into_string().map_err(|_| invalid())?,
        );
    }
    environment.extend([
        (
            "MISE_DATA_DIR".to_owned(),
            crate::runtime_paths::MISE_DATA_DIR.to_owned(),
        ),
        (
            "CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
    ]);
    if tools.contains(&PinnedTool::MrBoxington) || catalog.rust_uses_mbx() {
        let mbx = QualifiedDistribution::require_for_generator(
            DistributionTool::Mbx,
            host,
            DistributionRequirement::MbxTransport,
        )?;
        if catalog.rust_uses_mbx() {
            let relative_path = mbx.required_installed_binary_path()?;
            environment.extend([
                (
                    "MISE_OWNED_CARGO_WRAPPER".to_owned(),
                    format!("{}/{relative_path}", crate::runtime_paths::MISE_DATA_DIR),
                ),
                (
                    "MISE_OWNED_CARGO_WRAPPER_SHA256".to_owned(),
                    mbx.binary_sha256().to_owned(),
                ),
            ]);
        }
        qualifications.push(mbx);
    }
    let native_tools: Vec<_> = tools
        .iter()
        .copied()
        .filter(|tool| ToolCatalog::requires_native_host(*tool))
        .collect();
    qualifications.extend(catalog.qualified_distributions(host, &native_tools)?);
    super::super::native_tool_context::merge_native_environment(
        catalog,
        host,
        velnor_actions_contract::ToolCacheDomain::Full,
        &native_tools,
        &mut environment,
    )?;
    environment.insert(
        "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
        format!(
            "qualified-tools@{}",
            super::super::qualification::qualified_toolset_digest(&qualifications)
        ),
    );
    Ok(environment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::rust_desktop::RustCompilerRole;

    #[test]
    fn execution_overlay_never_invents_missing_owned_publications() -> Result<(), MiseError> {
        for role in [
            RustCompilerRole::RootLinux,
            RustCompilerRole::DesktopMac,
            RustCompilerRole::DesktopSourceMac,
            RustCompilerRole::ReleaseMac,
        ] {
            let catalog = super::super::catalog_for_role(role)?;
            assert!(
                qualified_exec_environment_for_tools(
                    &catalog,
                    distribution_host(role),
                    &[catalog.compiler_tool()],
                )
                .is_err()
            );
        }
        Ok(())
    }
}

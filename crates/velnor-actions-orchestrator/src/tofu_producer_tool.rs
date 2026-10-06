use velnor_actions_contract::{
    CompiledSourceHelper, PermissionLevel, Permissions, Step, StepId, ToolCacheDescriptor,
    ToolCacheDomain,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::MiseSetup;

use crate::OrchestratorError;

/// Build the producer's explicit no-permission token map.
pub(super) fn empty_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
        issues: PermissionLevel::None,
        pages: PermissionLevel::None,
        attestations: PermissionLevel::None,
    }
}

/// Derive the exact ToFu bootstrap payload from the compiled installer owner.
pub(super) fn preparation(
    catalog: &ToolCatalog,
    label: &str,
    target: &str,
    setup: &MiseSetup,
    version: &str,
) -> Result<(ToolCacheDescriptor, CompiledSourceHelper), OrchestratorError> {
    let host = host(target)?;
    let selectors = catalog.native_tool_specs(host, &[PinnedTool::Opentofu])?;
    let installation = velnor_actions_mise::catalog::tool_prepare::helper_for_tools(
        catalog,
        ToolCacheDomain::TofuBootstrap,
        host,
        &selectors,
        version,
    )
    .map_err(|error| OrchestratorError::Contract {
        problem: error.to_string(),
    })?;
    let descriptor = velnor_actions_workflow_renderer::tool_producer_steps::descriptor_for_record(
        &installation,
        label,
        target,
        ToolCacheDomain::TofuBootstrap,
        setup,
        std::slice::from_ref(&installation),
    )
    .map_err(OrchestratorError::from)?;
    Ok((descriptor, installation))
}

pub(super) fn preparation_step(record: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Prepare isolated Tofu source tools",
        record,
        record.environment().clone(),
    )?;
    step.id = Some(StepId::new("velnor-tofu-source-prepare")?);
    Ok(step)
}

fn host(
    target: &str,
) -> Result<velnor_actions_mise::catalog::qualification::DistributionHost, OrchestratorError> {
    use velnor_actions_mise::catalog::qualification::DistributionHost;
    match target {
        "x86_64-unknown-linux-gnu" => Ok(DistributionHost::LinuxAmd64),
        "aarch64-unknown-linux-gnu" => Ok(DistributionHost::LinuxArm64),
        "aarch64-apple-darwin" => Ok(DistributionHost::MacosArm64),
        _ => Err(OrchestratorError::Contract {
            problem: format!("unsupported_target:{target}"),
        }),
    }
}

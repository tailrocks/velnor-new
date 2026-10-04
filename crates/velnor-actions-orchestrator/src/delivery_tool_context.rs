//! Pure delivery-tool context from the compiled catalog and target Mise digest.

use velnor_actions_contract::VelnorConfig;
use velnor_actions_mise::catalog::{delivery_tools, qualification::DistributionHost};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::delivery_tools::DeliveryToolContext;

use crate::{OrchestratorError, pins::resolve_mise_setup};

/// Resolve exact native helper tools; unsupported runner targets fail closed.
pub(crate) fn delivery_tool_context(
    config: &VelnorConfig,
    runner_label: &str,
) -> Result<DeliveryToolContext, OrchestratorError> {
    let catalog = ToolCatalog::pinned();
    let host = match runner_label {
        "ubuntu-24.04" | "ubuntu-26.04" => DistributionHost::LinuxAmd64,
        "ubuntu-24.04-arm" | "ubuntu-26.04-arm" => DistributionHost::LinuxArm64,
        "macos-26" | "macos-15" => DistributionHost::MacosArm64,
        _ => {
            return Err(OrchestratorError::Contract {
                problem: format!("delivery_tools_unsupported_host:{runner_label}"),
            });
        }
    };
    let context = DeliveryToolContext {
        mise: resolve_mise_setup(config, runner_label)?,
        python_version: catalog.version(PinnedTool::Python).to_owned(),
        gh_version: catalog.version(PinnedTool::Gh).to_owned(),
        preparation: delivery_tools::preparation(host, env!("CARGO_PKG_VERSION"))?,
    };
    context.validate()?;
    Ok(context)
}

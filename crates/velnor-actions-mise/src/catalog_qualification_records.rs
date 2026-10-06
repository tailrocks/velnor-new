//! Official audit adapters use the standalone source-builder bootstrap authority.
//! Owned runtime records remain absent until publication and behavioral proof.

use super::super::source_build_bootstrap::{
    self, SourceBuildBootstrapFormat, SourceBuildBootstrapHost, SourceBuildBootstrapTool,
};
use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution,
};
use crate::MiseError;

pub(super) fn official(
    tool: DistributionTool,
    host: DistributionHost,
) -> Result<QualifiedDistribution, MiseError> {
    let source_tool = match tool {
        DistributionTool::Mise => SourceBuildBootstrapTool::Mise,
        DistributionTool::Mbx => SourceBuildBootstrapTool::Mbx,
        _ => return Err(absent(tool, host, "official installed-byte qualification")),
    };
    let source_host = match host {
        DistributionHost::LinuxAmd64 => SourceBuildBootstrapHost::LinuxAmd64,
        DistributionHost::LinuxArm64 => SourceBuildBootstrapHost::LinuxArm64,
        DistributionHost::MacosArm64 => SourceBuildBootstrapHost::MacosArm64,
    };
    let asset = source_build_bootstrap::official(source_tool, source_host);
    let asset_format = match asset.asset_format() {
        SourceBuildBootstrapFormat::Binary => DistributionAssetFormat::Binary,
        SourceBuildBootstrapFormat::TarGzip => DistributionAssetFormat::TarGzip,
    };
    QualifiedDistribution {
        tool,
        host,
        selector: asset.selector(),
        asset_url: asset.asset_url(),
        archive_sha256: asset.archive_sha256(),
        binary_sha256: asset.binary_sha256(),
        asset_format,
        binary_member: asset.binary_member(),
        source_repository: asset.source_repository(),
        source_commit: asset.source_commit(),
        source_tree: asset.source_tree(),
        owner: asset.owner(),
        version: asset.version(),
        selection_version: asset.version(),
        abi: asset.abi(),
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path: None,
        launch_entries: &[],
        source_lineage: &[],
        install_plan: None,
    }
    .validate()
}

pub(super) fn required(
    tool: DistributionTool,
    host: DistributionHost,
) -> Result<QualifiedDistribution, MiseError> {
    // No owned runtime publication is recorded. Never alias owned builds to upstream bytes.
    Err(absent(
        tool,
        host,
        "owned publication and behavioral qualification",
    ))
}

fn absent(tool: DistributionTool, host: DistributionHost, proof: &str) -> MiseError {
    MiseError::Contract {
        problem: format!(
            "qualified distribution absent: {tool:?} / {} requires {proof}",
            host.abi()
        ),
    }
}

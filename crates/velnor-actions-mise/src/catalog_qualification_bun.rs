//! Official Bun 1.4.2 release records for the closed host set.
//!
//! Evidence: `/tmp/velnor-tool-upgrades/bun-node/qualification-report.json`,
//! `/tmp/velnor-tool-upgrades/independent-source/verified-source-rows.json`,
//! and downloaded archives under `/tmp/velnor-bun-node-upgrade/candidate-assets/`.
//! GitHub release API digests match the downloaded ZIP bytes and extracted
//! `bun` members. Bun source commit `744846f844374847c902b5e7fd59b4342a51ef99`
//! has tree `22bf02488635ecf658474da0264016217f3064e8`.
//!
//! The Linux audit records retain the core selector `bun@1.4.2` and do not
//! publish an install layout. The macOS ARM64 record is qualified through the
//! exact HTTP selector exercised with Mise 2026.10.0: ZIP extraction strips the
//! one archive directory and installs `bun` at `installs/http-bun/1.4.2/bun`.
//! Mise verifies the archive before extraction; the installed Bun bytes were
//! hashed before the post-install version probe. Bun source defines the
//! embedded Node compatibility ABI as Node 26.3.0, module ABI 147, and N-API
//! 10.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

const VERSION: &str = "1.4.2";
const LINUX_SELECTOR: &str = "bun@1.4.2";
const MACOS_SELECTOR: &str = "http:bun[url=\"https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-darwin-aarch64.zip\",checksum=\"sha256:90987a3a16d7db556d886ac3d551e7b6d3edf0a1cf43acaed622e8676be1d12f\",strip_components=1]@1.4.2";
const OWNER: &str = "oven-sh/bun";
const SOURCE_REPOSITORY: &str = "https://github.com/oven-sh/bun";
const SOURCE_COMMIT: &str = "744846f844374847c902b5e7fd59b4342a51ef99";
const SOURCE_TREE: &str = "22bf02488635ecf658474da0264016217f3064e8";
const ABI: &str = "bun-v1.4.2;node-v26.3.0;modules-147;napi-10";
const TRANSFORM_ABI: &str = "bun-http-zip-strip1-root-bin-v1";

const BUN_LINEAGE: [QualifiedSourceLineage; 1] = [QualifiedSourceLineage {
    name: "bun",
    repository: SOURCE_REPOSITORY,
    commit: SOURCE_COMMIT,
    tree: SOURCE_TREE,
    version: VERSION,
}];

const LINUX_AMD64_LAUNCH: [QualifiedLaunchEntry; 1] = [QualifiedLaunchEntry {
    archive_member: "bun-linux-x64/bun",
    installed_relative_path: None,
    sha256: "a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c",
    kind: QualifiedLaunchKind::Executable,
}];

const LINUX_ARM64_LAUNCH: [QualifiedLaunchEntry; 1] = [QualifiedLaunchEntry {
    archive_member: "bun-linux-aarch64/bun",
    installed_relative_path: None,
    sha256: "616f267a34278ff5ac282df37ffdfba1d7141f4f6926bca99af2cd6ef3ad32b1",
    kind: QualifiedLaunchKind::Executable,
}];

const MACOS_ARM64_LAUNCH: [QualifiedLaunchEntry; 1] = [QualifiedLaunchEntry {
    archive_member: "bun-darwin-aarch64/bun",
    installed_relative_path: Some("installs/http-bun/1.4.2/bun"),
    sha256: "35d20dd0263e5c950194434b925454fdfa9ba6e4467da960410fa05b08a7a5b5",
    kind: QualifiedLaunchKind::Executable,
}];

static MACOS_ENVIRONMENT: [QualifiedInstallEnvironment; 1] = [QualifiedInstallEnvironment {
    name: "PATH",
    relative_path: "",
}];

static MACOS_INSTALL: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 1,
    bin_path: "",
    root_relative_path: "installs/http-bun/1.4.2",
    transform_abi: TRANSFORM_ABI,
    environment: &MACOS_ENVIRONMENT,
};

/// Build the immutable official record for the exact Bun selector.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION {
        return Err(MiseError::InvalidToolVersion {
            tool: "bun".to_owned(),
            version: version.to_owned(),
        });
    }
    let (
        asset_url,
        archive_sha256,
        binary_sha256,
        binary_member,
        launch_entries,
        install_plan,
        installed_binary_relative_path,
    ) = match host {
        DistributionHost::LinuxAmd64 => (
            "https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-linux-x64.zip",
            "36368faef7527875d5ffa52e53cd48021741f2a83eb6208a8dd64068d422a913",
            "a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c",
            "bun-linux-x64/bun",
            &LINUX_AMD64_LAUNCH,
            None,
            None,
        ),
        DistributionHost::LinuxArm64 => (
            "https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-linux-aarch64.zip",
            "54328bbc2d9c8e0c9f892c544d66c57a83b84139e34909e5ee81758f1ac8fda7",
            "616f267a34278ff5ac282df37ffdfba1d7141f4f6926bca99af2cd6ef3ad32b1",
            "bun-linux-aarch64/bun",
            &LINUX_ARM64_LAUNCH,
            None,
            None,
        ),
        DistributionHost::MacosArm64 => (
            "https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-darwin-aarch64.zip",
            "90987a3a16d7db556d886ac3d551e7b6d3edf0a1cf43acaed622e8676be1d12f",
            "35d20dd0263e5c950194434b925454fdfa9ba6e4467da960410fa05b08a7a5b5",
            "bun-darwin-aarch64/bun",
            &MACOS_ARM64_LAUNCH,
            Some(MACOS_INSTALL),
            Some("installs/http-bun/1.4.2/bun"),
        ),
    };
    let selector = match host {
        DistributionHost::LinuxAmd64 | DistributionHost::LinuxArm64 => LINUX_SELECTOR,
        DistributionHost::MacosArm64 => MACOS_SELECTOR,
    };
    QualifiedDistribution {
        tool: DistributionTool::Bun,
        host,
        selector,
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: DistributionAssetFormat::Zip,
        binary_member,
        source_repository: SOURCE_REPOSITORY,
        source_commit: SOURCE_COMMIT,
        source_tree: SOURCE_TREE,
        owner: OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi: ABI,
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path,
        launch_entries,
        source_lineage: &BUN_LINEAGE,
        install_plan,
    }
    .validate()
}

//! Official Node.js 24.21.0 release records for the closed host set.
//!
//! Evidence: `/tmp/velnor-tool-upgrades/bun-node/qualification-report.json`,
//! `/tmp/velnor-tool-upgrades/independent-source/verified-source-rows.json`,
//! and the downloaded archives under
//! `/tmp/velnor-bun-node-upgrade/candidate-assets/`. The Node release index and
//! SHASUMS rows were checked against downloaded `.tar.gz` bytes. The archive
//! contains Node 24.21.0 (module ABI 137) and npm 11.19.0; npm's public
//! registry tarball has the same npm and npx CLI bytes.
//!
//! The Linux audit records retain the core selector `node@24.21.0` and do not
//! publish an install layout. The macOS ARM64 record is qualified through the
//! exact HTTP selector exercised with Mise 2026.10.0: it strips one archive
//! component into `installs/http-node/24.21.0`, with `bin/node`, the generated
//! `bin/npm` shim, and the bundled npm CLI scripts under `lib/node_modules`.
//! Mise verifies the archive before extraction; installed Node, npm, and npx
//! bytes were hashed before post-install version probes.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

const VERSION: &str = "24.21.0";
const LINUX_SELECTOR: &str = "node@24.21.0";
const MACOS_SELECTOR: &str = "http:node[url=\"https://nodejs.org/dist/v24.21.0/node-v24.21.0-darwin-arm64.tar.gz\",checksum=\"sha256:bed7eea5325e1108f32ce5228ddd6a5f0f08a499ee42aa7442aea583702f6057\",strip_components=1,bin_path=\"bin\"]@24.21.0";
const OWNER: &str = "nodejs/node";
const SOURCE_REPOSITORY: &str = "https://github.com/nodejs/node";
const SOURCE_COMMIT: &str = "955266bfdd854cd280dffd47548673914484e4c0";
const SOURCE_TREE: &str = "4988d1b262c878669f7c32ddb3960752c97d3a10";
const ABI: &str = "node-v24.21.0;modules-137;napi-10";
const NPM_COMMIT: &str = "6a8a1b9ac4f530b67cc126ab93185aed1f742876";
const NPM_TREE: &str = "a5e78674cfae1f6c31654af59b6f9c2087a9d94a";
const NPM_REPOSITORY: &str = "https://github.com/npm/cli";
const NPM_VERSION: &str = "11.19.0";
const NPM_CLI_SHA256: &str = "8e5f6f3429f8cdbe693cdc29904e9d5a7b127a494bd15c804bd54c7403bfcbe7";
const NPX_CLI_SHA256: &str = "237adf8f3747cad8b9b62fcfd0d9c8d509a64e550337707f55100afcb79e8900";
const TRANSFORM_ABI: &str = "node-http-tar-gzip-strip1-bin-v1";

const NODE_LINEAGE: [QualifiedSourceLineage; 2] = [
    QualifiedSourceLineage {
        name: "node",
        repository: SOURCE_REPOSITORY,
        commit: SOURCE_COMMIT,
        tree: SOURCE_TREE,
        version: VERSION,
    },
    QualifiedSourceLineage {
        name: "bundled-npm",
        repository: NPM_REPOSITORY,
        commit: NPM_COMMIT,
        tree: NPM_TREE,
        version: NPM_VERSION,
    },
];

const LINUX_AMD64_LAUNCH: [QualifiedLaunchEntry; 3] = [
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-linux-x64/bin/node",
        installed_relative_path: None,
        sha256: "7fde7b8afa198da66257f42ee2001d874c7355631e6d1579a5fb5ef1f246df4c",
        kind: QualifiedLaunchKind::Executable,
    },
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-linux-x64/lib/node_modules/npm/bin/npm-cli.js",
        installed_relative_path: None,
        sha256: NPM_CLI_SHA256,
        kind: QualifiedLaunchKind::Script,
    },
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-linux-x64/lib/node_modules/npm/bin/npx-cli.js",
        installed_relative_path: None,
        sha256: NPX_CLI_SHA256,
        kind: QualifiedLaunchKind::Script,
    },
];

const LINUX_ARM64_LAUNCH: [QualifiedLaunchEntry; 3] = [
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-linux-arm64/bin/node",
        installed_relative_path: None,
        sha256: "0f8949d1028f6d61506b2d5bc57e7e6fe893d7b1997509b7847294fc9c616584",
        kind: QualifiedLaunchKind::Executable,
    },
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-linux-arm64/lib/node_modules/npm/bin/npm-cli.js",
        installed_relative_path: None,
        sha256: NPM_CLI_SHA256,
        kind: QualifiedLaunchKind::Script,
    },
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-linux-arm64/lib/node_modules/npm/bin/npx-cli.js",
        installed_relative_path: None,
        sha256: NPX_CLI_SHA256,
        kind: QualifiedLaunchKind::Script,
    },
];

const MACOS_ARM64_LAUNCH: [QualifiedLaunchEntry; 3] = [
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-darwin-arm64/bin/node",
        installed_relative_path: Some("installs/http-node/24.21.0/bin/node"),
        sha256: "e4b5a3af0e05c75de2eae013904145f40fe7fc2a6e6f17510128bf45cca4e79b",
        kind: QualifiedLaunchKind::Executable,
    },
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-darwin-arm64/lib/node_modules/npm/bin/npm-cli.js",
        installed_relative_path: Some(
            "installs/http-node/24.21.0/lib/node_modules/npm/bin/npm-cli.js",
        ),
        sha256: NPM_CLI_SHA256,
        kind: QualifiedLaunchKind::Script,
    },
    QualifiedLaunchEntry {
        archive_member: "node-v24.21.0-darwin-arm64/lib/node_modules/npm/bin/npx-cli.js",
        installed_relative_path: Some(
            "installs/http-node/24.21.0/lib/node_modules/npm/bin/npx-cli.js",
        ),
        sha256: NPX_CLI_SHA256,
        kind: QualifiedLaunchKind::Script,
    },
];

static MACOS_ENVIRONMENT: [QualifiedInstallEnvironment; 1] = [QualifiedInstallEnvironment {
    name: "PATH",
    relative_path: "bin",
}];

static MACOS_INSTALL: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 1,
    bin_path: "bin",
    root_relative_path: "installs/http-node/24.21.0",
    transform_abi: TRANSFORM_ABI,
    environment: &MACOS_ENVIRONMENT,
};

/// Build the immutable official record for the exact Node selector.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION {
        return Err(MiseError::InvalidToolVersion {
            tool: "node".to_owned(),
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
            "https://nodejs.org/dist/v24.21.0/node-v24.21.0-linux-x64.tar.gz",
            "6e1db87ef58b8819e5d5402eff1536491b18edd8eb7bee5ef7897876e88dc5ff",
            "7fde7b8afa198da66257f42ee2001d874c7355631e6d1579a5fb5ef1f246df4c",
            "node-v24.21.0-linux-x64/bin/node",
            &LINUX_AMD64_LAUNCH,
            None,
            None,
        ),
        DistributionHost::LinuxArm64 => (
            "https://nodejs.org/dist/v24.21.0/node-v24.21.0-linux-arm64.tar.gz",
            "724282c3b43aec998aa9527380465b45d229e021b58035f5f4f63095eabfe5d5",
            "0f8949d1028f6d61506b2d5bc57e7e6fe893d7b1997509b7847294fc9c616584",
            "node-v24.21.0-linux-arm64/bin/node",
            &LINUX_ARM64_LAUNCH,
            None,
            None,
        ),
        DistributionHost::MacosArm64 => (
            "https://nodejs.org/dist/v24.21.0/node-v24.21.0-darwin-arm64.tar.gz",
            "bed7eea5325e1108f32ce5228ddd6a5f0f08a499ee42aa7442aea583702f6057",
            "e4b5a3af0e05c75de2eae013904145f40fe7fc2a6e6f17510128bf45cca4e79b",
            "node-v24.21.0-darwin-arm64/bin/node",
            &MACOS_ARM64_LAUNCH,
            Some(MACOS_INSTALL),
            Some("installs/http-node/24.21.0/bin/node"),
        ),
    };
    let selector = match host {
        DistributionHost::LinuxAmd64 | DistributionHost::LinuxArm64 => LINUX_SELECTOR,
        DistributionHost::MacosArm64 => MACOS_SELECTOR,
    };
    QualifiedDistribution {
        tool: DistributionTool::Node,
        host,
        selector,
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: DistributionAssetFormat::TarGzip,
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
        source_lineage: &NODE_LINEAGE,
        install_plan,
    }
    .validate()
}

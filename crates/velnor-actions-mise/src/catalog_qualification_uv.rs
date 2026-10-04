//! Immutable uv release records for the supported Mise hosts.
//!
//! Release asset and extracted-launch hashes come from the immutable uv
//! `0.12.22` release. macOS ARM64 has the local runtime/workload proof; Linux
//! records bind public archive bytes and extracted launch bytes without making
//! an unmeasured local runtime claim.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

const VERSION: &str = "0.12.22";
const SOURCE_REPOSITORY: &str = "https://github.com/astral-sh/uv";
const SOURCE_OWNER: &str = "astral-sh/uv";
const SOURCE_COMMIT: &str = "70fe1196a546e49148a73b1c592b2f74c33af80e";
const SOURCE_TREE: &str = "87b1502eb7c676c9cc906e5a91cc0d13068fd27d";
const BUILD_DATE: &str = "2026-10-01";

const LINUX_AMD64_ASSET_URL: &str =
    "https://github.com/astral-sh/uv/releases/download/0.12.22/uv-x86_64-unknown-linux-gnu.tar.gz";
const LINUX_AMD64_ARCHIVE_SHA256: &str =
    "b9980552309f09c15172b8be828555e375097f16deb459795ce7bfd200380f0b";
const LINUX_AMD64_BINARY_SHA256: &str =
    "96e1603cb62aebb1a804fe9866a5708ee8bba4c39d07202ad29cf256a67366c3";
const LINUX_AMD64_UVX_SHA256: &str =
    "b2376fb87ddf103c44d4012218d509c0793521851b5af1d0c38f2441edb8b7a1";
const LINUX_ARM64_ASSET_URL: &str =
    "https://github.com/astral-sh/uv/releases/download/0.12.22/uv-aarch64-unknown-linux-gnu.tar.gz";
const LINUX_ARM64_ARCHIVE_SHA256: &str =
    "6f66a14e8239871fb477f9746c941fedfa77e8fe28a8bc7c07e1dc7f53a66712";
const LINUX_ARM64_BINARY_SHA256: &str =
    "4ad35befaffc486339a3ec0514e4e69050fe274126d997374a03d680960f7bf7";
const LINUX_ARM64_UVX_SHA256: &str =
    "c16bc61cb7c7becc15f1e3ed5701851bfbedf0d1f87c9f939dd75c6edfaf010f";
const MACOS_ASSET_URL: &str =
    "https://github.com/astral-sh/uv/releases/download/0.12.22/uv-aarch64-apple-darwin.tar.gz";
const MACOS_ARCHIVE_SHA256: &str =
    "5d714de09501a59393ceca78f4bc232a50478729640d251907160299b2a93ddd";
const MACOS_BINARY_SHA256: &str =
    "cf9ecd3dce5aed97bba9aa71b6217140eac437c6678412ba81a8af45ac6f1a4c";
const MACOS_UVX_SHA256: &str = "73f37488bffca116cc23ec676766a636c4b0dafc6ec00f60edb2278403b5af7e";
const MACOS_SELECTOR: &str = "http:uv[url=\"https://github.com/astral-sh/uv/releases/download/0.12.22/uv-aarch64-apple-darwin.tar.gz\",checksum=\"sha256:5d714de09501a59393ceca78f4bc232a50478729640d251907160299b2a93ddd\",strip_components=0,bin_path=\"uv-aarch64-apple-darwin\"]@0.12.22";
const TRANSFORM_ABI: &str = "uv-http-tar-gzip-strip0-bin-uv-aarch64-apple-darwin-v1";

static UV_LINEAGE: [QualifiedSourceLineage; 2] = [
    QualifiedSourceLineage {
        name: "uv",
        repository: SOURCE_REPOSITORY,
        commit: SOURCE_COMMIT,
        tree: SOURCE_TREE,
        version: VERSION,
    },
    QualifiedSourceLineage {
        name: "uv-build",
        repository: SOURCE_REPOSITORY,
        commit: SOURCE_COMMIT,
        tree: SOURCE_TREE,
        version: BUILD_DATE,
    },
];

static LINUX_AMD64_LAUNCH: [QualifiedLaunchEntry; 2] = [
    QualifiedLaunchEntry {
        archive_member: "uv-x86_64-unknown-linux-gnu/uv",
        installed_relative_path: None,
        sha256: LINUX_AMD64_BINARY_SHA256,
        kind: QualifiedLaunchKind::Executable,
    },
    QualifiedLaunchEntry {
        archive_member: "uv-x86_64-unknown-linux-gnu/uvx",
        installed_relative_path: None,
        sha256: LINUX_AMD64_UVX_SHA256,
        kind: QualifiedLaunchKind::Executable,
    },
];

static LINUX_ARM64_LAUNCH: [QualifiedLaunchEntry; 2] = [
    QualifiedLaunchEntry {
        archive_member: "uv-aarch64-unknown-linux-gnu/uv",
        installed_relative_path: None,
        sha256: LINUX_ARM64_BINARY_SHA256,
        kind: QualifiedLaunchKind::Executable,
    },
    QualifiedLaunchEntry {
        archive_member: "uv-aarch64-unknown-linux-gnu/uvx",
        installed_relative_path: None,
        sha256: LINUX_ARM64_UVX_SHA256,
        kind: QualifiedLaunchKind::Executable,
    },
];

static MACOS_LAUNCH: [QualifiedLaunchEntry; 2] = [
    QualifiedLaunchEntry {
        archive_member: "uv-aarch64-apple-darwin/uv",
        installed_relative_path: Some("installs/http-uv/0.12.22/uv-aarch64-apple-darwin/uv"),
        sha256: MACOS_BINARY_SHA256,
        kind: QualifiedLaunchKind::Executable,
    },
    QualifiedLaunchEntry {
        archive_member: "uv-aarch64-apple-darwin/uvx",
        installed_relative_path: Some("installs/http-uv/0.12.22/uv-aarch64-apple-darwin/uvx"),
        sha256: MACOS_UVX_SHA256,
        kind: QualifiedLaunchKind::Executable,
    },
];

static MACOS_ENVIRONMENT: [QualifiedInstallEnvironment; 1] = [QualifiedInstallEnvironment {
    name: "PATH",
    relative_path: "uv-aarch64-apple-darwin",
}];

static MACOS_INSTALL: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 0,
    bin_path: "uv-aarch64-apple-darwin",
    root_relative_path: "installs/http-uv/0.12.22",
    transform_abi: TRANSFORM_ABI,
    environment: &MACOS_ENVIRONMENT,
};

/// Build the immutable uv record for one supported host and exact version.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION {
        return Err(absent(host, version));
    }
    let (
        asset_url,
        archive_sha256,
        binary_sha256,
        binary_member,
        launch_entries,
        installed_binary_relative_path,
        install_plan,
        abi,
    ) = match host {
        DistributionHost::LinuxAmd64 => (
            LINUX_AMD64_ASSET_URL,
            LINUX_AMD64_ARCHIVE_SHA256,
            LINUX_AMD64_BINARY_SHA256,
            "uv-x86_64-unknown-linux-gnu/uv",
            &LINUX_AMD64_LAUNCH[..],
            None,
            None,
            "uv-0.12.22-x86_64-unknown-linux-gnu",
        ),
        DistributionHost::LinuxArm64 => (
            LINUX_ARM64_ASSET_URL,
            LINUX_ARM64_ARCHIVE_SHA256,
            LINUX_ARM64_BINARY_SHA256,
            "uv-aarch64-unknown-linux-gnu/uv",
            &LINUX_ARM64_LAUNCH[..],
            None,
            None,
            "uv-0.12.22-aarch64-unknown-linux-gnu",
        ),
        DistributionHost::MacosArm64 => (
            MACOS_ASSET_URL,
            MACOS_ARCHIVE_SHA256,
            MACOS_BINARY_SHA256,
            "uv-aarch64-apple-darwin/uv",
            &MACOS_LAUNCH[..],
            Some("installs/http-uv/0.12.22/uv-aarch64-apple-darwin/uv"),
            Some(MACOS_INSTALL),
            "uv-0.12.22-aarch64-apple-darwin",
        ),
    };
    let selector = match host {
        DistributionHost::LinuxAmd64 | DistributionHost::LinuxArm64 => "uv@0.12.22",
        DistributionHost::MacosArm64 => MACOS_SELECTOR,
    };
    QualifiedDistribution {
        tool: DistributionTool::Uv,
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
        owner: SOURCE_OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi,
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path,
        launch_entries,
        source_lineage: &UV_LINEAGE,
        install_plan,
    }
    .validate()
}

fn absent(host: DistributionHost, version: &str) -> MiseError {
    MiseError::Contract {
        problem: format!(
            "qualified distribution absent: Uv / {} / {version}",
            host.abi()
        ),
    }
}

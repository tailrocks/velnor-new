//! Audited official GitHub CLI bootstrap artifacts.
//!
//! This is the sole GH 2.102.0 bootstrap authority. The local evidence has
//! three supported archives and an actual macOS ARM64 HTTP installation.
//! Linux records retain source/archive evidence only; native launches fail closed.
//!
//! Evidence captured 2026-10-03 from the immutable release API record
//! `cli/cli/releases/399674740` (`v2.102.0`). The tag resolves to commit
//! `fc4b137cdef0a6bd28fd461b7cf9c84a5812a8cd`, tree
//! `a83338918c5283e049302d35e04ae8a01585c950`. Local archive bytes match both
//! the release API digests and `gh-binaries/checksums.txt`; each `bin/gh`
//! member was extracted and hashed without execution. macOS ZIP and installed
//! payload match across all 231 regular files; version/help probes succeeded.
//! Receipts: `/tmp/velnor-gh-native-qualification-20261003`. No Linux installation
//! transform or runtime qualification is inferred from the macOS measurement.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind,
};
use crate::MiseError;

const VERSION: &str = "2.102.0";
const LINUX_SELECTOR: &str = "github:cli/cli@2.102.0";
const MACOS_SELECTOR: &str = "http:gh[url=\"https://github.com/cli/cli/releases/download/v2.102.0/gh_2.102.0_macOS_arm64.zip\",checksum=\"sha256:da922c20d1792e5b2cbf375593d7a658acf034c12c84e007e71c76ef959c337e\",strip_components=1,bin_path=\"bin\"]@2.102.0";
const SOURCE_REPOSITORY: &str = "https://github.com/cli/cli";
const OWNER: &str = "cli/cli";
const SOURCE_COMMIT: &str = "fc4b137cdef0a6bd28fd461b7cf9c84a5812a8cd";
const SOURCE_TREE: &str = "a83338918c5283e049302d35e04ae8a01585c950";
const ABI: &str = "gh-cli-v2.102.0";

static LINUX_AMD64_LAUNCH: [QualifiedLaunchEntry; 1] = [QualifiedLaunchEntry {
    archive_member: "gh_2.102.0_linux_amd64/bin/gh",
    installed_relative_path: None,
    sha256: "7469124f706944133d6a169691dd1c6c3511b12e85878d255e044e2948df4c9b",
    kind: QualifiedLaunchKind::Executable,
}];

static LINUX_ARM64_LAUNCH: [QualifiedLaunchEntry; 1] = [QualifiedLaunchEntry {
    archive_member: "gh_2.102.0_linux_arm64/bin/gh",
    installed_relative_path: None,
    sha256: "93308395c2d296a63a662742c6366e4db413d2a4870d07bd9b84e491c065d65d",
    kind: QualifiedLaunchKind::Executable,
}];

const MACOS_LAUNCH: &[QualifiedLaunchEntry] = &[QualifiedLaunchEntry {
    archive_member: "gh_2.102.0_macOS_arm64/bin/gh",
    installed_relative_path: Some("installs/http-gh/2.102.0/bin/gh"),
    sha256: "8a4258433c81106343144857750316241759d06dcf16265cf3c4864a8f2f2ad6",
    kind: QualifiedLaunchKind::Executable,
}];

const MACOS_ENVIRONMENT: &[QualifiedInstallEnvironment] = &[QualifiedInstallEnvironment {
    name: "PATH",
    relative_path: "bin",
}];

const MACOS_INSTALL: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 1,
    bin_path: "bin",
    root_relative_path: "installs/http-gh/2.102.0",
    transform_abi: "mise-http-v2026.10.0-zip-strip1-bin",
    environment: MACOS_ENVIRONMENT,
};

/// Build the measured upstream GH record for the exact catalog version.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION {
        return Err(MiseError::InvalidToolVersion {
            tool: "gh".to_owned(),
            version: version.to_owned(),
        });
    }
    let (
        asset_url,
        archive_sha256,
        binary_sha256,
        binary_member,
        launch_entries,
        asset_format,
        install_plan,
        installed_path,
    ) = host_artifact(host);
    QualifiedDistribution {
        tool: DistributionTool::Gh,
        host,
        selector: if host == DistributionHost::MacosArm64 {
            MACOS_SELECTOR
        } else {
            LINUX_SELECTOR
        },
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: SOURCE_REPOSITORY,
        source_commit: SOURCE_COMMIT,
        source_tree: SOURCE_TREE,
        owner: OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi: ABI,
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path: installed_path,
        launch_entries,
        source_lineage: &[],
        install_plan,
    }
    .validate()
}

type HostArtifact = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [QualifiedLaunchEntry],
    DistributionAssetFormat,
    Option<QualifiedInstallPlan>,
    Option<&'static str>,
);

fn host_artifact(host: DistributionHost) -> HostArtifact {
    match host {
        DistributionHost::LinuxAmd64 => (
            "https://github.com/cli/cli/releases/download/v2.102.0/gh_2.102.0_linux_amd64.tar.gz",
            "bb766f710eef8ede859c18578c72c327597cd4c8a85b06001b1f3843c6019386",
            "7469124f706944133d6a169691dd1c6c3511b12e85878d255e044e2948df4c9b",
            "gh_2.102.0_linux_amd64/bin/gh",
            &LINUX_AMD64_LAUNCH[..],
            DistributionAssetFormat::TarGzip,
            None,
            None,
        ),
        DistributionHost::LinuxArm64 => (
            "https://github.com/cli/cli/releases/download/v2.102.0/gh_2.102.0_linux_arm64.tar.gz",
            "7862c86c72f43df3a2d93ddde6f473285b4e2af61b494849846827e513ef6484",
            "93308395c2d296a63a662742c6366e4db413d2a4870d07bd9b84e491c065d65d",
            "gh_2.102.0_linux_arm64/bin/gh",
            &LINUX_ARM64_LAUNCH[..],
            DistributionAssetFormat::TarGzip,
            None,
            None,
        ),
        DistributionHost::MacosArm64 => (
            "https://github.com/cli/cli/releases/download/v2.102.0/gh_2.102.0_macOS_arm64.zip",
            "da922c20d1792e5b2cbf375593d7a658acf034c12c84e007e71c76ef959c337e",
            "8a4258433c81106343144857750316241759d06dcf16265cf3c4864a8f2f2ad6",
            "gh_2.102.0_macOS_arm64/bin/gh",
            MACOS_LAUNCH,
            DistributionAssetFormat::Zip,
            Some(MACOS_INSTALL),
            Some("installs/http-gh/2.102.0/bin/gh"),
        ),
    }
}

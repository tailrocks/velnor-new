//! Official release-plz 0.3.169 native CLI records.
//!
//! The release-plz coordinator is an upstream executable used by the
//! source-only Rust release profile. It is separate from the owned fixed
//! publisher implementation and does not qualify that publisher. Evidence was
//! captured from the immutable GitHub release `release-plz-v0.3.169`, whose
//! annotated tag resolves to commit `786894b6ce1abad0d0e9bea0ace4958e099af8ca`
//! and tree `7aab0f02c3b5789fe94d06677e652945ca97832c`. The three supported
//! archives were downloaded, safely extracted, and their regular `release-plz`
//! members hashed without execution. The macOS ARM64 archive was also
//! installed through the exact Mise HTTP selector; the installed bytes were
//! hashed before `--version` and `--help` probes.
//!
//! The publisher probe reported `release-plz 0.3.169` and the expected
//! version/help command surface. Linux records intentionally carry archive and
//! source evidence only; no Linux installed-path or HTTP transform is inferred.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

/// Exact release-plz catalog version.
pub(super) const VERSION: &str = "0.3.169";
/// Canonical release-plz source repository.
pub(super) const SOURCE_REPOSITORY: &str = "https://github.com/release-plz/release-plz";
/// Exact source commit resolved from the annotated release tag.
pub(super) const SOURCE_COMMIT: &str = "786894b6ce1abad0d0e9bea0ace4958e099af8ca";
/// Exact source tree resolved from [`SOURCE_COMMIT`].
pub(super) const SOURCE_TREE: &str = "7aab0f02c3b5789fe94d06677e652945ca97832c";

const OWNER: &str = "release-plz/release-plz";
const LINUX_SELECTOR: &str = "release-plz@0.3.169";
const MACOS_SELECTOR: &str = "http:release-plz[url=\"https://github.com/release-plz/release-plz/releases/download/release-plz-v0.3.169/release-plz-aarch64-apple-darwin.tar.gz\",checksum=\"sha256:3bb728a921e0f9d6aca48723de2d8a49a71099ca8a113efb9350781120648a7e\",strip_components=0]@0.3.169";
const BINARY_MEMBER: &str = "release-plz";
const ABI: &str = "release-plz-cli-v0.3.169";
const TRANSFORM_ABI: &str = "mise-http-v2026.10.0-tar-gz-strip0-root-bin";

const MACOS_ASSET_URL: &str = "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.3.169/release-plz-aarch64-apple-darwin.tar.gz";
const MACOS_ARCHIVE_SHA256: &str =
    "3bb728a921e0f9d6aca48723de2d8a49a71099ca8a113efb9350781120648a7e";
const MACOS_BINARY_SHA256: &str =
    "139d0364919dac7a6acddc4688a6a388cfa6cf2c239bc64a9c5cef53c8e43927";

const LINUX_AMD64_ASSET_URL: &str = "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.3.169/release-plz-x86_64-unknown-linux-gnu.tar.gz";
const LINUX_AMD64_ARCHIVE_SHA256: &str =
    "1455106d3263712dd796ee04869eaee98b274e1e2e18011f67682b1f2840b64d";
const LINUX_AMD64_BINARY_SHA256: &str =
    "ce1a27f086fb5cff2019aa8f91357f39579955f4be645cb9c953f7028cb53dee";

const LINUX_ARM64_ASSET_URL: &str = "https://github.com/release-plz/release-plz/releases/download/release-plz-v0.3.169/release-plz-aarch64-unknown-linux-gnu.tar.gz";
const LINUX_ARM64_ARCHIVE_SHA256: &str =
    "6d4ce34d00342a7b10557cf03a893f8d68737063c8f49faaed26ae54daf2a4c6";
const LINUX_ARM64_BINARY_SHA256: &str =
    "84948a64adcd29fb9bc8ef1da614825296322776b5572c65b006130fc3a0c93b";

const SOURCE_LINEAGE: &[QualifiedSourceLineage] = &[];

const LINUX_AMD64_LAUNCH: &[QualifiedLaunchEntry] = &[QualifiedLaunchEntry {
    archive_member: BINARY_MEMBER,
    installed_relative_path: None,
    sha256: LINUX_AMD64_BINARY_SHA256,
    kind: QualifiedLaunchKind::Executable,
}];

const LINUX_ARM64_LAUNCH: &[QualifiedLaunchEntry] = &[QualifiedLaunchEntry {
    archive_member: BINARY_MEMBER,
    installed_relative_path: None,
    sha256: LINUX_ARM64_BINARY_SHA256,
    kind: QualifiedLaunchKind::Executable,
}];

const MACOS_LAUNCH: &[QualifiedLaunchEntry] = &[QualifiedLaunchEntry {
    archive_member: BINARY_MEMBER,
    installed_relative_path: Some("installs/http-release-plz/0.3.169/release-plz"),
    sha256: MACOS_BINARY_SHA256,
    kind: QualifiedLaunchKind::Executable,
}];

const MACOS_ENVIRONMENT: &[QualifiedInstallEnvironment] = &[QualifiedInstallEnvironment {
    name: "PATH",
    relative_path: "",
}];

const MACOS_INSTALL: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 0,
    bin_path: "",
    root_relative_path: "installs/http-release-plz/0.3.169",
    transform_abi: TRANSFORM_ABI,
    environment: MACOS_ENVIRONMENT,
};

/// Build the official release-plz record for one exact supported host.
///
/// Linux has verified archive/source evidence but no installed-path receipt.
/// macOS ARM64 carries the measured Mise HTTP transform and CLI probe.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION {
        return Err(MiseError::InvalidToolVersion {
            tool: "release-plz".to_owned(),
            version: version.to_owned(),
        });
    }
    let (asset_url, archive_sha256, binary_sha256, launch_entries, install_plan, installed_path) =
        match host {
            DistributionHost::LinuxAmd64 => (
                LINUX_AMD64_ASSET_URL,
                LINUX_AMD64_ARCHIVE_SHA256,
                LINUX_AMD64_BINARY_SHA256,
                LINUX_AMD64_LAUNCH,
                None,
                None,
            ),
            DistributionHost::LinuxArm64 => (
                LINUX_ARM64_ASSET_URL,
                LINUX_ARM64_ARCHIVE_SHA256,
                LINUX_ARM64_BINARY_SHA256,
                LINUX_ARM64_LAUNCH,
                None,
                None,
            ),
            DistributionHost::MacosArm64 => (
                MACOS_ASSET_URL,
                MACOS_ARCHIVE_SHA256,
                MACOS_BINARY_SHA256,
                MACOS_LAUNCH,
                Some(MACOS_INSTALL),
                Some("installs/http-release-plz/0.3.169/release-plz"),
            ),
        };
    let selector = match host {
        DistributionHost::LinuxAmd64 | DistributionHost::LinuxArm64 => LINUX_SELECTOR,
        DistributionHost::MacosArm64 => MACOS_SELECTOR,
    };
    QualifiedDistribution {
        tool: DistributionTool::ReleasePlz,
        host,
        selector,
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: DistributionAssetFormat::TarGzip,
        binary_member: BINARY_MEMBER,
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
        source_lineage: SOURCE_LINEAGE,
        install_plan,
    }
    .validate()
}

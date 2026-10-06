//! Official `OpenTofu` 1.13.1 distribution records.
//!
//! The release API, official `tofu_1.13.1_SHA256SUMS`, and independently
//! fetched archives agreed on 2026-10-03. Each archive was inspected before
//! extraction; the only executable member is the regular file `tofu`.
//!
//! Source identity was resolved through the official GitHub APIs:
//! annotated tag `c512d0956f274dfbe02de09ccb7b534f16caf13e`, commit
//! `233700b795b6d2372f1c56ed57a4abaec9d36475`, tree
//! `4233427bb88ebdcc5fff857455dbead78d37db60`.
//! Mise registry proof: the ordinary selector resolves through
//! `aqua:opentofu/opentofu`; this record uses the explicit HTTP selector below
//! so its URL, archive SHA, and extraction transform stay qualified.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallPlan, QualifiedLaunchEntry,
    QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

const VERSION: &str = "1.13.1";
const SOURCE_REPOSITORY: &str = "https://github.com/opentofu/opentofu";
const SOURCE_COMMIT: &str = "233700b795b6d2372f1c56ed57a4abaec9d36475";
const SOURCE_TREE: &str = "4233427bb88ebdcc5fff857455dbead78d37db60";
const OWNER: &str = "opentofu/opentofu";
const MACOS_INSTALL_PATH: &str = "installs/http-opentofu/1.13.1/tofu";
const MACOS_ROOT: &str = "installs/http-opentofu/1.13.1";
const TRANSFORM_ABI: &str = "opentofu-http-strip0-root-v1";

const LINUX_AMD64_URL: &str =
    "https://github.com/opentofu/opentofu/releases/download/v1.13.1/tofu_1.13.1_linux_amd64.tar.gz";
const LINUX_AMD64_ARCHIVE_SHA256: &str =
    "378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69";
const LINUX_AMD64_SELECTOR: &str = "http:opentofu[url=\"https://github.com/opentofu/opentofu/releases/download/v1.13.1/tofu_1.13.1_linux_amd64.tar.gz\",checksum=\"sha256:378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69\",strip_components=0]@1.13.1";
const LINUX_ARM64_URL: &str =
    "https://github.com/opentofu/opentofu/releases/download/v1.13.1/tofu_1.13.1_linux_arm64.tar.gz";
const LINUX_ARM64_ARCHIVE_SHA256: &str =
    "9c1ef375aa1852db0b2888aa921b640c71f8140d4682aa4fec99378a64fa7dc3";
const LINUX_ARM64_SELECTOR: &str = "http:opentofu[url=\"https://github.com/opentofu/opentofu/releases/download/v1.13.1/tofu_1.13.1_linux_arm64.tar.gz\",checksum=\"sha256:9c1ef375aa1852db0b2888aa921b640c71f8140d4682aa4fec99378a64fa7dc3\",strip_components=0]@1.13.1";
const MACOS_ARM64_URL: &str = "https://github.com/opentofu/opentofu/releases/download/v1.13.1/tofu_1.13.1_darwin_arm64.tar.gz";
const MACOS_ARM64_ARCHIVE_SHA256: &str =
    "be78f659f04ef06a9dbd9b3934d46af95d787a3aa38396d459dea395261816a9";
const MACOS_ARM64_SELECTOR: &str = "http:opentofu[url=\"https://github.com/opentofu/opentofu/releases/download/v1.13.1/tofu_1.13.1_darwin_arm64.tar.gz\",checksum=\"sha256:be78f659f04ef06a9dbd9b3934d46af95d787a3aa38396d459dea395261816a9\",strip_components=0]@1.13.1";

const SOURCE_LINEAGE: &[QualifiedSourceLineage] = &[QualifiedSourceLineage {
    name: "opentofu",
    repository: SOURCE_REPOSITORY,
    commit: SOURCE_COMMIT,
    tree: SOURCE_TREE,
    version: VERSION,
}];

// The archive member and extracted byte hashes are measured for every host.
// The installed path and HTTP plan are Some only for the isolated macOS Mise
// probe; no Linux host-side installation was executed in this qualification
// lane. The macOS probe used the exact selector above and a clean MISE_DATA_DIR.
const LINUX_AMD64_LAUNCH: &[QualifiedLaunchEntry] = &[QualifiedLaunchEntry {
    archive_member: "tofu",
    installed_relative_path: None,
    sha256: "a325c8c2f6834575e440b03c2ba67f94256072754ac7787fea718be6f01fef6a",
    kind: QualifiedLaunchKind::Executable,
}];
const LINUX_ARM64_LAUNCH: &[QualifiedLaunchEntry] = &[QualifiedLaunchEntry {
    archive_member: "tofu",
    installed_relative_path: None,
    sha256: "d5c690023baebe8bf2cfb2062e292186f4797a173db22c973bd9de45cb68a62f",
    kind: QualifiedLaunchKind::Executable,
}];
const MACOS_ARM64_LAUNCH: &[QualifiedLaunchEntry] = &[QualifiedLaunchEntry {
    archive_member: "tofu",
    installed_relative_path: Some(MACOS_INSTALL_PATH),
    sha256: "56b267df2cbe6498d6b1deedb9cbff038954dfc20830dfd51abea658fabbe586",
    kind: QualifiedLaunchKind::Executable,
}];

const MACOS_INSTALL: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 0,
    bin_path: "",
    root_relative_path: MACOS_ROOT,
    transform_abi: TRANSFORM_ABI,
    environment: &[],
};

/// Build the measured upstream `OpenTofu` record for one supported host.
///
/// `version` is an explicit input so callers cannot silently substitute a
/// newer release. The current catalog has one measured release only.
///
/// # Errors
///
/// Rejects a version other than the measured release or any changed static
/// identity field.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION {
        return Err(MiseError::Contract {
            problem: format!("unsupported OpenTofu qualification version: {version}"),
        });
    }
    let (asset_url, archive_sha256, binary_sha256, selector, launch_entries, install_plan) =
        match host {
            DistributionHost::LinuxAmd64 => (
                LINUX_AMD64_URL,
                LINUX_AMD64_ARCHIVE_SHA256,
                "a325c8c2f6834575e440b03c2ba67f94256072754ac7787fea718be6f01fef6a",
                LINUX_AMD64_SELECTOR,
                LINUX_AMD64_LAUNCH,
                None,
            ),
            DistributionHost::LinuxArm64 => (
                LINUX_ARM64_URL,
                LINUX_ARM64_ARCHIVE_SHA256,
                "d5c690023baebe8bf2cfb2062e292186f4797a173db22c973bd9de45cb68a62f",
                LINUX_ARM64_SELECTOR,
                LINUX_ARM64_LAUNCH,
                None,
            ),
            DistributionHost::MacosArm64 => (
                MACOS_ARM64_URL,
                MACOS_ARM64_ARCHIVE_SHA256,
                "56b267df2cbe6498d6b1deedb9cbff038954dfc20830dfd51abea658fabbe586",
                MACOS_ARM64_SELECTOR,
                MACOS_ARM64_LAUNCH,
                Some(MACOS_INSTALL),
            ),
        };
    QualifiedDistribution {
        tool: DistributionTool::OpenTofu,
        host,
        selector,
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: DistributionAssetFormat::TarGzip,
        binary_member: "tofu",
        source_repository: SOURCE_REPOSITORY,
        source_commit: SOURCE_COMMIT,
        source_tree: SOURCE_TREE,
        owner: OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi: host.abi(),
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path: launch_entries
            .first()
            .and_then(QualifiedLaunchEntry::installed_relative_path),
        launch_entries,
        source_lineage: SOURCE_LINEAGE,
        install_plan,
    }
    .validate()
}

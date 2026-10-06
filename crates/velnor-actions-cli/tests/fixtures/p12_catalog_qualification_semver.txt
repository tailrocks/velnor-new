//! Canonical Cargo Semver Checks 0.50.0 distribution record.
//!
//! The descriptor and raw receipt under `/tmp/nativeauthority/cargo-semver-checks`
//! are the sole admitted HTTP qualification evidence. The receipt compares the
//! downloaded archive member with the installed regular file byte-for-byte.
//! Linux assets have archive evidence elsewhere, but no qualified Linux HTTP
//! installation record is emitted until the host-specific authority is bound.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

/// Exact catalog selection version adopted by the parent tool pin.
pub(super) const VERSION: &str = "0.50.0";

const SELECTOR: &str = "http:cargo-semver-checks[url=\"https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v0.50.0/cargo-semver-checks-aarch64-apple-darwin.tar.gz\",checksum=\"sha256:f99f928d67501c29e8410026f27a54cf799fb13611c3b4a6a58f2772cf7e3799\",strip_components=0]@0.50.0";
const ASSET_URL: &str = "https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v0.50.0/cargo-semver-checks-aarch64-apple-darwin.tar.gz";
const ARCHIVE_SHA256: &str = "f99f928d67501c29e8410026f27a54cf799fb13611c3b4a6a58f2772cf7e3799";
const BINARY_SHA256: &str = "9ce8bc0cd0a5f0f1aa8b5728d9745104adf774fb84970847cc9901b55f61a3a1";
pub(super) const SOURCE_REPOSITORY: &str = "https://github.com/obi1kenobi/cargo-semver-checks";
pub(super) const SOURCE_COMMIT: &str = "4297e8b5f6306531375ba2ba332171e5792b4c38";
const SOURCE_TREE: &str = "4f640b40228b0162141eb1d10961b4c1b5029189";
const OWNER: &str = "obi1kenobi/cargo-semver-checks";
const BINARY_MEMBER: &str = "cargo-semver-checks";
const INSTALL_ROOT: &str = "installs/http-cargo-semver-checks/0.50.0";
const INSTALL_PATH: &str = "installs/http-cargo-semver-checks/0.50.0/cargo-semver-checks";
const TRANSFORM_ABI: &str = "mise-http-v2026.10.0-tar.gz-strip0-root-bin";

const SOURCE_LINEAGE: &[QualifiedSourceLineage] = &[];

const LAUNCH_ENTRIES: &[QualifiedLaunchEntry] = &[QualifiedLaunchEntry {
    archive_member: BINARY_MEMBER,
    installed_relative_path: Some(INSTALL_PATH),
    sha256: BINARY_SHA256,
    kind: QualifiedLaunchKind::Executable,
}];

const INSTALL_ENVIRONMENT: &[QualifiedInstallEnvironment] = &[QualifiedInstallEnvironment {
    name: "PATH",
    relative_path: "",
}];

const INSTALL_PLAN: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 0,
    bin_path: "",
    root_relative_path: INSTALL_ROOT,
    transform_abi: TRANSFORM_ABI,
    environment: INSTALL_ENVIRONMENT,
};

/// Build the canonical HTTP qualification for the measured host.
///
/// Linux is deliberately absent: this factory cannot promote archive-only
/// evidence into an installed-path qualification. The publisher version and
/// command help probes are evidence of the selected executable, while actual
/// semver execution remains outside this record's scope.
///
/// # Errors
///
/// Rejects every version except the exact adopted pin and every host except
/// the measured macOS ARM64 HTTP installation.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION {
        return Err(MiseError::Contract {
            problem: format!("unsupported Cargo Semver Checks version: {version}"),
        });
    }
    if host != DistributionHost::MacosArm64 {
        return Err(MiseError::Contract {
            problem: format!(
                "qualified Cargo Semver Checks installation absent: {} / {version}",
                host.abi()
            ),
        });
    }
    QualifiedDistribution {
        tool: DistributionTool::CargoSemverChecks,
        host,
        selector: SELECTOR,
        asset_url: ASSET_URL,
        archive_sha256: ARCHIVE_SHA256,
        binary_sha256: BINARY_SHA256,
        asset_format: DistributionAssetFormat::TarGzip,
        binary_member: BINARY_MEMBER,
        source_repository: SOURCE_REPOSITORY,
        source_commit: SOURCE_COMMIT,
        source_tree: SOURCE_TREE,
        owner: OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi: host.abi(),
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path: Some(INSTALL_PATH),
        launch_entries: LAUNCH_ENTRIES,
        source_lineage: SOURCE_LINEAGE,
        install_plan: Some(INSTALL_PLAN),
    }
    .validate()
}

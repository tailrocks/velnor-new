//! Audited Gradle 9.4.1 wrapper bootstrap distribution.
//!
//! This role is the source-qualified bootstrap authority for the consumer
//! wrapper. It is distinct from the consumer engine (9.5.1) and the managed
//! Gradle distribution (9.8.0). The archive, Gradle build receipt/source
//! identity, and isolated macOS HTTP installation were measured separately.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

/// Exact Gradle release used for the wrapper bootstrap role.
pub(super) const VERSION: &str = "9.4.1";

const ASSET_URL: &str = "https://downloads.gradle.org/distributions/gradle-9.4.1-bin.zip";
const ARCHIVE_SHA256: &str = "2ab2958f2a1e51120c326cad6f385153bb11ee93b3c216c5fccebfdfbb7ec6cb";
const SOURCE_REPOSITORY: &str = "https://github.com/gradle/gradle";
const SOURCE_COMMIT: &str = "2d6327017519d23b96af35865dc997fcb544fb40";
const SOURCE_TREE: &str = "9013a215c2d94d029236608ec2c1457579e33ef6";
const OWNER: &str = "gradle/gradle";
const INSTALL_ROOT: &str = "installs/http-gradle/9.4.1";
const BIN_PATH: &str = "bin";
const TRANSFORM_ABI: &str = "gradle-wrapper-bootstrap-http-strip1-bin-v1";
const SCRIPT_PATH: &str = "installs/http-gradle/9.4.1/bin/gradle";
const LAUNCHER_PATH: &str = "installs/http-gradle/9.4.1/lib/gradle-launcher-9.4.1.jar";
const CLI_MAIN_PATH: &str = "installs/http-gradle/9.4.1/lib/gradle-gradle-cli-main-9.4.1.jar";
const WRAPPER_SHARED_PATH: &str = "installs/http-gradle/9.4.1/lib/gradle-wrapper-shared-9.4.1.jar";
const WRAPPER_MAIN_PATH: &str =
    "installs/http-gradle/9.4.1/lib/plugins/gradle-wrapper-main-9.4.1.jar";

const SOURCE_LINEAGE: &[QualifiedSourceLineage] = &[QualifiedSourceLineage {
    name: "gradle-wrapper-bootstrap",
    repository: SOURCE_REPOSITORY,
    commit: SOURCE_COMMIT,
    tree: SOURCE_TREE,
    version: VERSION,
}];

const MACOS_LAUNCH_ENTRIES: &[QualifiedLaunchEntry] = &[
    QualifiedLaunchEntry {
        archive_member: "gradle-9.4.1/bin/gradle",
        installed_relative_path: Some(SCRIPT_PATH),
        sha256: "d995438d2583976188015a46642bd2e66f7aec23d1a1bf00267a6a27e4a4155b",
        kind: QualifiedLaunchKind::Script,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.4.1/lib/gradle-launcher-9.4.1.jar",
        installed_relative_path: Some(LAUNCHER_PATH),
        sha256: "541d9a671246033cae467215bdfc16839958888f3808a6aa937ba7fc802945ed",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.4.1/lib/gradle-gradle-cli-main-9.4.1.jar",
        installed_relative_path: Some(CLI_MAIN_PATH),
        sha256: "bbd7e9e1f7d7e9ce75afe724c00be485241f53fc20fc4e4a3adad157b815072b",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.4.1/lib/gradle-wrapper-shared-9.4.1.jar",
        installed_relative_path: Some(WRAPPER_SHARED_PATH),
        sha256: "4e208eff4b63c24b9b6a3ec05ccc5f7edffce5b771f832d17a445c0593fe6677",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.4.1/lib/plugins/gradle-wrapper-main-9.4.1.jar",
        installed_relative_path: Some(WRAPPER_MAIN_PATH),
        sha256: "9219345804fe690d4e7fe4c6404ad56b2cdcab225e7782db429ef1238e53d91f",
        kind: QualifiedLaunchKind::JavaArchive,
    },
];

const ENVIRONMENT: &[QualifiedInstallEnvironment] = &[QualifiedInstallEnvironment {
    name: "PATH",
    relative_path: BIN_PATH,
}];

const INSTALL_PLAN: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 1,
    bin_path: BIN_PATH,
    root_relative_path: INSTALL_ROOT,
    transform_abi: TRANSFORM_ABI,
    environment: ENVIRONMENT,
};

/// Build the measured wrapper bootstrap distribution for one exact version.
///
/// Linux remains absent because only the isolated macOS HTTP installation was
/// executed; archive bytes alone cannot provide an installed bootstrap path.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION || host != DistributionHost::MacosArm64 {
        return Err(absent(host, version));
    }
    QualifiedDistribution {
        tool: DistributionTool::Gradle,
        host,
        selector: selector(),
        asset_url: ASSET_URL,
        archive_sha256: ARCHIVE_SHA256,
        binary_sha256: MACOS_LAUNCH_ENTRIES[0].sha256,
        asset_format: DistributionAssetFormat::Zip,
        binary_member: "gradle-9.4.1/bin/gradle",
        source_repository: SOURCE_REPOSITORY,
        source_commit: SOURCE_COMMIT,
        source_tree: SOURCE_TREE,
        owner: OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi: "gradle-wrapper-bootstrap-distribution-9.4.1",
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path: Some(SCRIPT_PATH),
        launch_entries: MACOS_LAUNCH_ENTRIES,
        source_lineage: SOURCE_LINEAGE,
        install_plan: Some(INSTALL_PLAN),
    }
    .validate()
}

fn selector() -> &'static str {
    concat!(
        "http:gradle",
        "[url=\"https://downloads.gradle.org/distributions/gradle-9.4.1-bin.zip\",checksum=\"sha256:2ab2958f2a1e51120c326cad6f385153bb11ee93b3c216c5fccebfdfbb7ec6cb\",strip_components=1,bin_path=\"bin\"]@9.4.1"
    )
}

fn absent(host: DistributionHost, version: &str) -> MiseError {
    MiseError::Contract {
        problem: format!(
            "qualified Gradle wrapper bootstrap absent: {} / {version}",
            host.abi()
        ),
    }
}

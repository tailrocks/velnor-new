//! Audited Gradle 9.5.1 consumer distribution.
//!
//! This is the engine selected by the repository's consumer wrapper. It is a
//! separate authority from the managed Gradle distribution and from the
//! Gradle 9.4.1 wrapper bootstrap. The isolated HTTP receipt measured the
//! macOS install and the script plus JVM launcher/wrapper JAR closure.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

/// Exact engine selected by the consumer wrapper.
pub(super) const VERSION: &str = "9.5.1";

const ASSET_URL: &str = "https://downloads.gradle.org/distributions/gradle-9.5.1-bin.zip";
const ARCHIVE_SHA256: &str = "bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f";
const SOURCE_REPOSITORY: &str = "https://github.com/gradle/gradle";
const SOURCE_COMMIT: &str = "fd78213f09782e62ca4957f9cfd3d90c6c3f1767";
const SOURCE_TREE: &str = "0897e48ad4e4b80d7039a86f59a0cd9056436dd3";
const OWNER: &str = "gradle/gradle";
const INSTALL_ROOT: &str = "installs/http-gradle/9.5.1";
const BIN_PATH: &str = "bin";
const TRANSFORM_ABI: &str = "gradle-consumer-http-strip1-bin-v1";
const SCRIPT_PATH: &str = "installs/http-gradle/9.5.1/bin/gradle";
const LAUNCHER_PATH: &str = "installs/http-gradle/9.5.1/lib/gradle-launcher-9.5.1.jar";
const CLI_MAIN_PATH: &str = "installs/http-gradle/9.5.1/lib/gradle-gradle-cli-main-9.5.1.jar";
const WRAPPER_SHARED_PATH: &str = "installs/http-gradle/9.5.1/lib/gradle-wrapper-shared-9.5.1.jar";
const WRAPPER_MAIN_PATH: &str =
    "installs/http-gradle/9.5.1/lib/plugins/gradle-wrapper-main-9.5.1.jar";

const SOURCE_LINEAGE: &[QualifiedSourceLineage] = &[QualifiedSourceLineage {
    name: "gradle-consumer",
    repository: SOURCE_REPOSITORY,
    commit: SOURCE_COMMIT,
    tree: SOURCE_TREE,
    version: VERSION,
}];

const MACOS_LAUNCH_ENTRIES: &[QualifiedLaunchEntry] = &[
    QualifiedLaunchEntry {
        archive_member: "gradle-9.5.1/bin/gradle",
        installed_relative_path: Some(SCRIPT_PATH),
        sha256: "4c66ca7ace807bd6706308a195f44dff00ce4af9a793c0929a696dc9659c82c9",
        kind: QualifiedLaunchKind::Script,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.5.1/lib/gradle-launcher-9.5.1.jar",
        installed_relative_path: Some(LAUNCHER_PATH),
        sha256: "f04e6e1d414ca5df373fbd05ab9c4f692456a6544e9498eb4ccfc092a98944b8",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.5.1/lib/gradle-gradle-cli-main-9.5.1.jar",
        installed_relative_path: Some(CLI_MAIN_PATH),
        sha256: "08a21dd9402d244fb4e5a98f1a7e427a84603d5095c2e641fe6763efb535e14e",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.5.1/lib/gradle-wrapper-shared-9.5.1.jar",
        installed_relative_path: Some(WRAPPER_SHARED_PATH),
        sha256: "418540a8921a65f5b27e6bf6aff629e92b4fcff646da01ea9d8a96cb1d36760b",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.5.1/lib/plugins/gradle-wrapper-main-9.5.1.jar",
        installed_relative_path: Some(WRAPPER_MAIN_PATH),
        sha256: "ca76d2c841167f9ff4b87d3e77fa570375ea8375538328afaac1df179d700457",
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

/// Build the measured consumer distribution for one exact catalog version.
///
/// Linux has no installed-path HTTP receipt in this qualification lane, so it
/// remains absent even though the ZIP is platform-independent.
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
        binary_member: "gradle-9.5.1/bin/gradle",
        source_repository: SOURCE_REPOSITORY,
        source_commit: SOURCE_COMMIT,
        source_tree: SOURCE_TREE,
        owner: OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi: "gradle-consumer-distribution-9.5.1",
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
        "[url=\"https://downloads.gradle.org/distributions/gradle-9.5.1-bin.zip\",checksum=\"sha256:bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f\",strip_components=1,bin_path=\"bin\"]@9.5.1"
    )
}

fn absent(host: DistributionHost, version: &str) -> MiseError {
    MiseError::Contract {
        problem: format!(
            "qualified Gradle consumer distribution absent: {} / {version}",
            host.abi()
        ),
    }
}

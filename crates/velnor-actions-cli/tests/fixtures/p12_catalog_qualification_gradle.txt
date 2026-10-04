//! Audited Gradle 9.8.0 distribution and JVM launcher records.
//!
//! The official services distribution was downloaded and checked against its
//! published SHA256. The bundled shell entry point, JVM launcher, and wrapper
//! JAR inputs were hashed from the ZIP members. The Mac HTTP probe measured
//! the extracted root and launch paths. Consumer wrapper/bootstrap transport
//! remains a separate authority.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

pub(super) const VERSION: &str = "9.8.0";
const ASSET_URL: &str = "https://services.gradle.org/distributions/gradle-9.8.0-bin.zip";
const ARCHIVE_SHA256: &str = "bafd5ce9cfaea0fbccfdc8439a1ac42fbd4cd9c89dc9a988228d8a2639a58e6c";
const SOURCE_REPOSITORY: &str = "https://github.com/gradle/gradle";
const SOURCE_COMMIT: &str = "a927be5e08efe79e0b87ada06c762dde6bb9f8b8";
const SOURCE_TREE: &str = "56fedb055e013a3bfddde6905d6a7adad1047398";
const OWNER: &str = "gradle/gradle";
const INSTALL_ROOT: &str = "installs/http-gradle/9.8.0";
const BIN_PATH: &str = "bin";
const TRANSFORM_ABI: &str = "gradle-http-strip1-bin-v1";
const SCRIPT_PATH: &str = "installs/http-gradle/9.8.0/bin/gradle";
const LAUNCHER_PATH: &str = "installs/http-gradle/9.8.0/lib/gradle-launcher-9.8.0.jar";
const WRAPPER_SHARED_PATH: &str = "installs/http-gradle/9.8.0/lib/gradle-wrapper-shared-9.8.0.jar";
const WRAPPER_MAIN_PATH: &str =
    "installs/http-gradle/9.8.0/lib/plugins/gradle-wrapper-main-9.8.0.jar";
const SCRIPT_SHA256: &str = "f1a69bc070c613e5818d469a1248c1833519dd096d6828bb6fa4cde76de769e0";

const SOURCE_LINEAGE: &[QualifiedSourceLineage] = &[QualifiedSourceLineage {
    name: "gradle",
    repository: SOURCE_REPOSITORY,
    commit: SOURCE_COMMIT,
    tree: SOURCE_TREE,
    version: VERSION,
}];

const AUDIT_LAUNCH_ENTRIES: &[QualifiedLaunchEntry] = &[
    QualifiedLaunchEntry {
        archive_member: "gradle-9.8.0/bin/gradle",
        installed_relative_path: None,
        sha256: SCRIPT_SHA256,
        kind: QualifiedLaunchKind::Script,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.8.0/lib/gradle-launcher-9.8.0.jar",
        installed_relative_path: None,
        sha256: "248bc2a7473b67a892e47c8f8c916ade133e128a28ad55134424f1b6f598602a",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.8.0/lib/gradle-wrapper-shared-9.8.0.jar",
        installed_relative_path: None,
        sha256: "7854057f0440ed04d6b1c05cb1faa9189f6e64671f554c9731e17b9f5deaea02",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.8.0/lib/plugins/gradle-wrapper-main-9.8.0.jar",
        installed_relative_path: None,
        sha256: "0942d3eada1a143e4f52dacd1ba111a631a4a5e18d16ca6b68c99acbfb5ba938",
        kind: QualifiedLaunchKind::JavaArchive,
    },
];

const MACOS_LAUNCH_ENTRIES: &[QualifiedLaunchEntry] = &[
    QualifiedLaunchEntry {
        archive_member: "gradle-9.8.0/bin/gradle",
        installed_relative_path: Some(SCRIPT_PATH),
        sha256: SCRIPT_SHA256,
        kind: QualifiedLaunchKind::Script,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.8.0/lib/gradle-launcher-9.8.0.jar",
        installed_relative_path: Some(LAUNCHER_PATH),
        sha256: "248bc2a7473b67a892e47c8f8c916ade133e128a28ad55134424f1b6f598602a",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.8.0/lib/gradle-wrapper-shared-9.8.0.jar",
        installed_relative_path: Some(WRAPPER_SHARED_PATH),
        sha256: "7854057f0440ed04d6b1c05cb1faa9189f6e64671f554c9731e17b9f5deaea02",
        kind: QualifiedLaunchKind::JavaArchive,
    },
    QualifiedLaunchEntry {
        archive_member: "gradle-9.8.0/lib/plugins/gradle-wrapper-main-9.8.0.jar",
        installed_relative_path: Some(WRAPPER_MAIN_PATH),
        sha256: "0942d3eada1a143e4f52dacd1ba111a631a4a5e18d16ca6b68c99acbfb5ba938",
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

/// Build the measured Gradle distribution for one exact catalog version.
pub(super) fn official(
    host: DistributionHost,
    version: &str,
) -> Result<QualifiedDistribution, MiseError> {
    if version != VERSION {
        return Err(absent(host, version));
    }
    let (launch_entries, installed_binary_relative_path, install_plan) = match host {
        DistributionHost::MacosArm64 => {
            (MACOS_LAUNCH_ENTRIES, Some(SCRIPT_PATH), Some(INSTALL_PLAN))
        }
        DistributionHost::LinuxAmd64 | DistributionHost::LinuxArm64 => {
            (AUDIT_LAUNCH_ENTRIES, None, None)
        }
    };
    QualifiedDistribution {
        tool: DistributionTool::Gradle,
        host,
        selector: selector(),
        asset_url: ASSET_URL,
        archive_sha256: ARCHIVE_SHA256,
        binary_sha256: SCRIPT_SHA256,
        asset_format: DistributionAssetFormat::Zip,
        binary_member: "gradle-9.8.0/bin/gradle",
        source_repository: SOURCE_REPOSITORY,
        source_commit: SOURCE_COMMIT,
        source_tree: SOURCE_TREE,
        owner: OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi: "gradle-distribution-9.8.0",
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path,
        launch_entries,
        source_lineage: SOURCE_LINEAGE,
        install_plan,
    }
    .validate()
}

fn selector() -> &'static str {
    concat!(
        "http:gradle",
        "[url=\"https://services.gradle.org/distributions/gradle-9.8.0-bin.zip\",checksum=\"sha256:bafd5ce9cfaea0fbccfdc8439a1ac42fbd4cd9c89dc9a988228d8a2639a58e6c\",strip_components=1,bin_path=\"bin\"]@9.8.0"
    )
}

fn absent(host: DistributionHost, version: &str) -> MiseError {
    MiseError::Contract {
        problem: format!(
            "qualified distribution absent: Gradle / {} / {version}",
            host.abi()
        ),
    }
}

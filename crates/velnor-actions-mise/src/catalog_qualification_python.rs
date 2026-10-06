//! Immutable CPython/PBS records for the supported Mise Python route.
//!
//! `CPython` source lineage and the python-build-standalone provider are kept as
//! separate identities. The provider release supplies the executable bytes;
//! the `CPython` tag supplies the upstream language source identity.

use super::{
    DistributionAssetFormat, DistributionHost, DistributionTool, ProvisioningMode,
    QualifiedDistribution, QualifiedInstallBackend, QualifiedInstallEnvironment,
    QualifiedInstallPlan, QualifiedLaunchEntry, QualifiedLaunchKind, QualifiedSourceLineage,
};
use crate::MiseError;

const VERSION: &str = "3.14.8";
const PROVIDER_REPOSITORY: &str = "https://github.com/astral-sh/python-build-standalone";
const PROVIDER_OWNER: &str = "astral-sh/python-build-standalone";
const PROVIDER_RELEASE: &str = "20261001";
const PROVIDER_COMMIT: &str = "63249f9a31f23542d5a58754aed0b93cc432e761";
const PROVIDER_TREE: &str = "94b604e87a621ef4559d5908787165fa93607788";
const PROVIDER_BUILD_DATE: &str = "2026-10-01";
const CPYTHON_REPOSITORY: &str = "https://github.com/python/cpython";
const CPYTHON_COMMIT: &str = "8e6e75d9102e39bed2a2b279203a396741180f12";
const CPYTHON_TREE: &str = "5c846838d075d0d44c37549e11cdc334fbef2054";
const CPYTHON_LINEAGE_NAME: &str = "cpython";
const PROVIDER_LINEAGE_NAME: &str = "python-build-standalone";
const TRANSFORM_ABI: &str = "python-http-pbs-strip1-bin-v1";

const MACOS_ARCHIVE_URL: &str = "https://github.com/astral-sh/python-build-standalone/releases/download/20261001/cpython-3.14.8%2B20261001-aarch64-apple-darwin-install_only_stripped.tar.gz";
const MACOS_ARCHIVE_SHA256: &str =
    "69e48cd7f54261df5b6abbd374f69fe6e496fa781d271592aeef49cd5ffbea6c";
const MACOS_BINARY_SHA256: &str =
    "02f23beea109ccb3ad0f4b211bbea0810b912a94840de34374ad4a8c04f32222";
const LINUX_AMD64_ARCHIVE_URL: &str = "https://github.com/astral-sh/python-build-standalone/releases/download/20261001/cpython-3.14.8%2B20261001-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz";
const LINUX_AMD64_ARCHIVE_SHA256: &str =
    "b373a4a4e4e70fc05f368c9b53d7738bf37637682b650d96c742805d2da26c32";
const LINUX_AMD64_BINARY_SHA256: &str =
    "4b67d7e58e4e3f58339106f9192dbd66421608dc2fcc6a705b20115ba588232b";
const LINUX_ARM64_ARCHIVE_URL: &str = "https://github.com/astral-sh/python-build-standalone/releases/download/20261001/cpython-3.14.8%2B20261001-aarch64-unknown-linux-gnu-install_only_stripped.tar.gz";
const LINUX_ARM64_ARCHIVE_SHA256: &str =
    "4395ae16388f9d3409cba7e20161753ed3359b13d959345415029f874f675162";
const LINUX_ARM64_BINARY_SHA256: &str =
    "dd175b863222f46cdea72f460377b41b213cd623990ac0d68b22a715b8e5b9bd";
const LINUX_AMD64_SELECTOR: &str = "http:python[url=\"https://github.com/astral-sh/python-build-standalone/releases/download/20261001/cpython-3.14.8%2B20261001-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz\",checksum=\"sha256:b373a4a4e4e70fc05f368c9b53d7738bf37637682b650d96c742805d2da26c32\",strip_components=1,bin_path=\"bin\"]@3.14.8";
const LINUX_ARM64_SELECTOR: &str = "http:python[url=\"https://github.com/astral-sh/python-build-standalone/releases/download/20261001/cpython-3.14.8%2B20261001-aarch64-unknown-linux-gnu-install_only_stripped.tar.gz\",checksum=\"sha256:4395ae16388f9d3409cba7e20161753ed3359b13d959345415029f874f675162\",strip_components=1,bin_path=\"bin\"]@3.14.8";
const MACOS_SELECTOR: &str = "http:python[url=\"https://github.com/astral-sh/python-build-standalone/releases/download/20261001/cpython-3.14.8%2B20261001-aarch64-apple-darwin-install_only_stripped.tar.gz\",checksum=\"sha256:69e48cd7f54261df5b6abbd374f69fe6e496fa781d271592aeef49cd5ffbea6c\",strip_components=1,bin_path=\"bin\"]@3.14.8";

static PYTHON_LINEAGE: [QualifiedSourceLineage; 3] = [
    QualifiedSourceLineage {
        name: CPYTHON_LINEAGE_NAME,
        repository: CPYTHON_REPOSITORY,
        commit: CPYTHON_COMMIT,
        tree: CPYTHON_TREE,
        version: VERSION,
    },
    QualifiedSourceLineage {
        name: PROVIDER_LINEAGE_NAME,
        repository: PROVIDER_REPOSITORY,
        commit: PROVIDER_COMMIT,
        tree: PROVIDER_TREE,
        version: PROVIDER_RELEASE,
    },
    QualifiedSourceLineage {
        name: "python-build-standalone-build",
        repository: PROVIDER_REPOSITORY,
        commit: PROVIDER_COMMIT,
        tree: PROVIDER_TREE,
        version: PROVIDER_BUILD_DATE,
    },
];

static MACOS_ENVIRONMENT: [QualifiedInstallEnvironment; 2] = [
    QualifiedInstallEnvironment {
        name: "PATH",
        relative_path: "bin",
    },
    QualifiedInstallEnvironment {
        name: "PYTHONHOME",
        relative_path: "",
    },
];

static MACOS_LAUNCH: [QualifiedLaunchEntry; 1] = [QualifiedLaunchEntry {
    archive_member: "python/bin/python3.14",
    installed_relative_path: Some("installs/http-python/3.14.8/bin/python3.14"),
    sha256: MACOS_BINARY_SHA256,
    kind: QualifiedLaunchKind::Executable,
}];

static LINUX_AMD64_LAUNCH: [QualifiedLaunchEntry; 1] = [QualifiedLaunchEntry {
    archive_member: "python/bin/python3.14",
    installed_relative_path: None,
    sha256: LINUX_AMD64_BINARY_SHA256,
    kind: QualifiedLaunchKind::Executable,
}];

static LINUX_ARM64_LAUNCH: [QualifiedLaunchEntry; 1] = [QualifiedLaunchEntry {
    archive_member: "python/bin/python3.14",
    installed_relative_path: None,
    sha256: LINUX_ARM64_BINARY_SHA256,
    kind: QualifiedLaunchKind::Executable,
}];

static MACOS_INSTALL: QualifiedInstallPlan = QualifiedInstallPlan {
    backend: QualifiedInstallBackend::MiseHttp,
    strip_components: 1,
    bin_path: "bin",
    root_relative_path: "installs/http-python/3.14.8",
    transform_abi: TRANSFORM_ABI,
    environment: &MACOS_ENVIRONMENT,
};

/// Build the immutable Python record for one supported host and exact version.
///
/// Linux assets have verified public archive and extracted-launch hashes. The
/// local runtime relocation proof covers macOS ARM64, so only that host carries
/// an installed path and HTTP install plan.
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
        launch_entries,
        install_plan,
        installed_binary_relative_path,
        abi,
    ) = match host {
        DistributionHost::LinuxAmd64 => (
            LINUX_AMD64_ARCHIVE_URL,
            LINUX_AMD64_ARCHIVE_SHA256,
            LINUX_AMD64_BINARY_SHA256,
            &LINUX_AMD64_LAUNCH[..],
            None,
            None,
            "cpython-3.14.8-x86_64-unknown-linux-gnu",
        ),
        DistributionHost::LinuxArm64 => (
            LINUX_ARM64_ARCHIVE_URL,
            LINUX_ARM64_ARCHIVE_SHA256,
            LINUX_ARM64_BINARY_SHA256,
            &LINUX_ARM64_LAUNCH[..],
            None,
            None,
            "cpython-3.14.8-aarch64-unknown-linux-gnu",
        ),
        DistributionHost::MacosArm64 => (
            MACOS_ARCHIVE_URL,
            MACOS_ARCHIVE_SHA256,
            MACOS_BINARY_SHA256,
            &MACOS_LAUNCH[..],
            Some(MACOS_INSTALL),
            Some("installs/http-python/3.14.8/bin/python3.14"),
            "cpython-3.14.8-aarch64-apple-darwin",
        ),
    };
    let selector = match host {
        DistributionHost::LinuxAmd64 => LINUX_AMD64_SELECTOR,
        DistributionHost::LinuxArm64 => LINUX_ARM64_SELECTOR,
        DistributionHost::MacosArm64 => MACOS_SELECTOR,
    };
    QualifiedDistribution {
        tool: DistributionTool::Python,
        host,
        selector,
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: DistributionAssetFormat::TarGzip,
        binary_member: "python/bin/python3.14",
        source_repository: PROVIDER_REPOSITORY,
        source_commit: PROVIDER_COMMIT,
        source_tree: PROVIDER_TREE,
        owner: PROVIDER_OWNER,
        version: VERSION,
        selection_version: VERSION,
        abi,
        provisioning_mode: ProvisioningMode::Official,
        installed_binary_relative_path,
        launch_entries,
        source_lineage: &PYTHON_LINEAGE,
        install_plan,
    }
    .validate()
}

fn absent(host: DistributionHost, version: &str) -> MiseError {
    MiseError::Contract {
        problem: format!(
            "qualified distribution absent: Python / {} / {version}",
            host.abi()
        ),
    }
}

//! Closed official source-builder bootstrap assets.
//!
//! This authority owns only upstream bootstrap bytes used to acquire the
//! source-build path. It is separate from runtime distribution qualification:
//! an asset here does not establish Velnor-owned behavior or installation.
//!
//! Mise evidence was captured 2026-10-20 from release `v2026.10.15`; the
//! official source commit is `597c1897dfed28d1c0dea9f98fba581059e42cd3` with
//! tree `4786f2724d63cb59e06226bde8d3d58c09cb80dd`. MBX evidence was captured
//! from release `v1.25.0`; its source commit is
//! `2ade2bc6b661f1b40a55cfa7083fa9a4efc9348d` with tree
//! `54170d2d1f8fa99bbd7f1bbce328cd69562e6305`. Official archive digests
//! matched fetched bytes; extracted executable members were hashed without execution.

/// Closed upstream bootstrap tool identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBuildBootstrapTool {
    /// Official Mise bootstrap executable.
    Mise,
    /// Official Mr. Boxington bootstrap executable.
    Mbx,
}

/// Closed hosts with measured official bootstrap assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBuildBootstrapHost {
    /// GNU Linux on AMD64.
    LinuxAmd64,
    /// GNU Linux on ARM64.
    LinuxArm64,
    /// Darwin on ARM64.
    MacosArm64,
}

/// Container format of an official bootstrap asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBuildBootstrapFormat {
    /// Standalone executable bytes.
    Binary,
    /// Gzip-compressed tar archive.
    TarGzip,
}

/// Immutable official bootstrap asset record.
///
/// Fields remain private so callers can obtain records only through the
/// closed [`official`] factory and read them through the accessors below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceBuildBootstrapAsset {
    tool: SourceBuildBootstrapTool,
    host: SourceBuildBootstrapHost,
    selector: &'static str,
    asset_url: &'static str,
    archive_sha256: &'static str,
    binary_sha256: &'static str,
    asset_format: SourceBuildBootstrapFormat,
    binary_member: &'static str,
    source_repository: &'static str,
    source_commit: &'static str,
    source_tree: &'static str,
    owner: &'static str,
    version: &'static str,
    abi: &'static str,
}

impl SourceBuildBootstrapAsset {
    /// Exact upstream selector.
    #[must_use]
    pub const fn selector(&self) -> &'static str {
        self.selector
    }

    /// Official asset URL.
    #[must_use]
    pub const fn asset_url(&self) -> &'static str {
        self.asset_url
    }

    /// SHA-256 of the downloaded asset bytes.
    #[must_use]
    pub const fn archive_sha256(&self) -> &'static str {
        self.archive_sha256
    }

    /// SHA-256 of the admitted executable bytes.
    #[must_use]
    pub const fn binary_sha256(&self) -> &'static str {
        self.binary_sha256
    }

    /// Asset container format.
    #[must_use]
    pub const fn asset_format(&self) -> SourceBuildBootstrapFormat {
        self.asset_format
    }

    /// Explicit executable member; empty for standalone binaries.
    #[must_use]
    pub const fn binary_member(&self) -> &'static str {
        self.binary_member
    }

    /// Exact upstream source repository.
    #[must_use]
    pub const fn source_repository(&self) -> &'static str {
        self.source_repository
    }

    /// Exact upstream source commit.
    #[must_use]
    pub const fn source_commit(&self) -> &'static str {
        self.source_commit
    }

    /// Exact upstream source tree.
    #[must_use]
    pub const fn source_tree(&self) -> &'static str {
        self.source_tree
    }

    /// Upstream owner identity.
    #[must_use]
    pub const fn owner(&self) -> &'static str {
        self.owner
    }

    /// Upstream release version.
    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }

    /// Explicit bootstrap executable ABI.
    #[must_use]
    pub const fn abi(&self) -> &'static str {
        self.abi
    }

    /// Bootstrap tool identity.
    #[must_use]
    pub const fn tool(&self) -> SourceBuildBootstrapTool {
        self.tool
    }

    /// Bootstrap host identity.
    #[must_use]
    pub const fn host(&self) -> SourceBuildBootstrapHost {
        self.host
    }
}

/// Return the measured official bootstrap asset for a closed tool/host pair.
#[must_use]
pub const fn official(
    tool: SourceBuildBootstrapTool,
    host: SourceBuildBootstrapHost,
) -> SourceBuildBootstrapAsset {
    match tool {
        SourceBuildBootstrapTool::Mise => mise(host),
        SourceBuildBootstrapTool::Mbx => mbx(host),
    }
}

const fn mise(host: SourceBuildBootstrapHost) -> SourceBuildBootstrapAsset {
    let (asset_url, archive_sha256, binary_sha256, asset_format, binary_member) = match host {
        SourceBuildBootstrapHost::LinuxAmd64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.15/mise-v2026.10.15-linux-x64",
            "e3b186d4c70627f27037be615becaf4acfb30e41fb6a1819486717dc94e4e2a9",
            "e3b186d4c70627f27037be615becaf4acfb30e41fb6a1819486717dc94e4e2a9",
            SourceBuildBootstrapFormat::Binary,
            "",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.15/mise-v2026.10.15-linux-arm64.tar.gz",
            "ae36e72ea72d337ab7b367edb0f7016fa1502aa0e36ccb705acad7902c60677b",
            "3abbaac305a465ef038eb4ea07b961c037a8b639108005e43f8e2c00a8f67c11",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.15/mise-v2026.10.15-macos-arm64.tar.gz",
            "8a44a56f72323c777298a1d5abd88ffd21d0fca19fab1ccccd389a912374f69c",
            "826c4833fa7437a63f6027b86bc10c8b61b1163dc793c03ecde92f68ad167890",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mise,
        host,
        selector: "github:jdx/mise@2026.10.15",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: "https://github.com/jdx/mise",
        source_commit: "597c1897dfed28d1c0dea9f98fba581059e42cd3",
        source_tree: "4786f2724d63cb59e06226bde8d3d58c09cb80dd",
        owner: "jdx/mise",
        version: "2026.10.15",
        abi: "mise-cli-v2026.10.15",
    }
}

const fn mbx(host: SourceBuildBootstrapHost) -> SourceBuildBootstrapAsset {
    let (asset_url, archive_sha256, binary_sha256) = match host {
        SourceBuildBootstrapHost::LinuxAmd64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.25.0/mbx-x86_64-unknown-linux-gnu.tar.gz",
            "2b4efb2ef340d38ed2d840d32b5535e45be7541224378ff353c7a83f85744195",
            "48424ca46f5f3b984f0d8b0c2dcafaaaaf8dbde50464b6ecfcef1b324bfc251d",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.25.0/mbx-aarch64-unknown-linux-gnu.tar.gz",
            "26adcf79262395e7fb355203afa7deda0f18fd790585f6e49e51994b9d32b55c",
            "aef1baabde6a1eb6299fcf03c33e61a15538426aa7e82c9204e5ff7419ec1c3b",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.25.0/mbx-aarch64-apple-darwin.tar.gz",
            "47630813d15a001cfceceeeb1f35008421fb22b8ba132106067d217e000b1b68",
            "2d957002c399d77670c7222fc4475c021adacfa0b63eb709af9437b63de08e2d",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mbx,
        host,
        selector: "github:jdx/mr-boxington@1.25.0",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: SourceBuildBootstrapFormat::TarGzip,
        binary_member: "mbx",
        source_repository: "https://github.com/jdx/mr-boxington",
        source_commit: "2ade2bc6b661f1b40a55cfa7083fa9a4efc9348d",
        source_tree: "54170d2d1f8fa99bbd7f1bbce328cd69562e6305",
        owner: "jdx/mr-boxington",
        version: "1.25.0",
        abi: "mbx-cli-v1.25.0",
    }
}

#[cfg(test)]
#[path = "catalog_source_build_bootstrap_tests.rs"]
mod tests;

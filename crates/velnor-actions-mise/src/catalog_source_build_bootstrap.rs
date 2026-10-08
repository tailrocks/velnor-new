//! Closed official source-builder bootstrap assets.
//!
//! This authority owns only upstream bootstrap bytes used to acquire the
//! source-build path. It is separate from runtime distribution qualification:
//! an asset here does not establish Velnor-owned behavior or installation.
//!
//! Mise evidence was captured 2026-10-08 from release `v2026.10.4`; the
//! official source commit is `96cca90d3e55519a47cffa0cb99baa4c3d3ecca3` with
//! tree `92dda3fb668211ebaa2cf4edd832a184526ee918`. MBX evidence was captured
//! from release `v1.21.1`; its source commit is
//! `a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313` with tree
//! `1158c764f3893bacbd3a2f3e51990a9de1cb3712`. Downloaded archive bytes and
//! extracted executable members were hashed without execution.

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
            "https://github.com/jdx/mise/releases/download/v2026.10.4/mise-v2026.10.4-linux-x64",
            "2b8ce21f550872807bcaabf45b6bc5c64bfbd6dc3bf49dd4e67de700ef3ceb75",
            "2b8ce21f550872807bcaabf45b6bc5c64bfbd6dc3bf49dd4e67de700ef3ceb75",
            SourceBuildBootstrapFormat::Binary,
            "",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.4/mise-v2026.10.4-linux-arm64.tar.gz",
            "8760841cdbf964ecf9902a50c94716c77185a99af7f8eb55c9c51ec73ecd8880",
            "9013ce1d7d9bbbf65254cda178562f5450c474a705907c18b77e6b678bb10041",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.4/mise-v2026.10.4-macos-arm64.tar.gz",
            "744ae45f9b7c2a443adfa61df48397930e88b13c541834b7bd22ca31d4dfcfcd",
            "5c530143fc750e8a98c9a36be8d361e5dd953fa0b004d58f7577783f7cf2ac24",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mise,
        host,
        selector: "github:jdx/mise@2026.10.4",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: "https://github.com/jdx/mise",
        source_commit: "96cca90d3e55519a47cffa0cb99baa4c3d3ecca3",
        source_tree: "92dda3fb668211ebaa2cf4edd832a184526ee918",
        owner: "jdx/mise",
        version: "2026.10.4",
        abi: "mise-cli-v2026.10.4",
    }
}

const fn mbx(host: SourceBuildBootstrapHost) -> SourceBuildBootstrapAsset {
    let (asset_url, archive_sha256, binary_sha256) = match host {
        SourceBuildBootstrapHost::LinuxAmd64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.21.1/mbx-x86_64-unknown-linux-gnu.tar.gz",
            "1ecb4d55582a40a1227e8ca3450da054ea464e5ff976bb5762943fb1ce6f31da",
            "97984b8c92953cefc027014d24c8abf8773f2d12ded0385156d2050da8d5fb8c",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.21.1/mbx-aarch64-unknown-linux-gnu.tar.gz",
            "a783ff78192a3cd299cfbf2b4b8a8dc16b8142c7bb962e9e3a027b64085189b8",
            "e39ab5b1617c9ac72108058d899d931c8d2f553a0e6ba31b95509c8b79260df4",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.21.1/mbx-aarch64-apple-darwin.tar.gz",
            "99464a5bad96c3a472714faa4277aac22193ea9a385dd09892f5c1bbee9c56ba",
            "ed67908a8661b84fea1ad41f70ed502b77cbcc704ea90918f16b2ce7045408dc",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mbx,
        host,
        selector: "github:jdx/mr-boxington@1.21.1",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: SourceBuildBootstrapFormat::TarGzip,
        binary_member: "mbx",
        source_repository: "https://github.com/jdx/mr-boxington",
        source_commit: "a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313",
        source_tree: "1158c764f3893bacbd3a2f3e51990a9de1cb3712",
        owner: "jdx/mr-boxington",
        version: "1.21.1",
        abi: "mbx-cli-v1.21.1",
    }
}

#[cfg(test)]
#[path = "catalog_source_build_bootstrap_tests.rs"]
mod tests;

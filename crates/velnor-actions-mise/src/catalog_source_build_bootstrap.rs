//! Closed official source-builder bootstrap assets.
//!
//! This authority owns only upstream bootstrap bytes used to acquire the
//! source-build path. It is separate from runtime distribution qualification:
//! an asset here does not establish Velnor-owned behavior or installation.
//!
//! Mise evidence was captured 2026-10-19 from release `v2026.10.14`; the
//! official source commit is `d7131a7d69a2bdb2cfc84a66a5ff97329dd740e9` with
//! tree `86511d143b7a9b3af5f2f43ed6cab6e60f03c72c`. MBX evidence was captured
//! from release `v1.24.1`; its source commit is
//! `ebb2b61b193d9ea487b67152d459e8332e8cf87e` with tree
//! `9a416015df0ce5db6fc05b41e20de65f75f0915d`. Downloaded archive bytes and
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
            "https://github.com/jdx/mise/releases/download/v2026.10.14/mise-v2026.10.14-linux-x64",
            "50be7185bcf75929f01cc5630276bcb43ca367f7c5b176e2580903d28c611dc4",
            "50be7185bcf75929f01cc5630276bcb43ca367f7c5b176e2580903d28c611dc4",
            SourceBuildBootstrapFormat::Binary,
            "",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.14/mise-v2026.10.14-linux-arm64.tar.gz",
            "ff76e1d99fc27bcb2b3b38d1afd0798a785088aad93cc4206f2227195d2063cd",
            "a9a8eec284d1c8e1a76685dcfa2fd609e76773fe56546eb72e4568dd1f50c1c3",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.14/mise-v2026.10.14-macos-arm64.tar.gz",
            "eb68c824e16af90f61e70e0d785179f335b1b95466606a5e0d8b9da504bb7bb2",
            "005b32b9c22ca9cb63ff9d35c74b3abe69c433e9ca655fea288ba3dcbe7e4918",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mise,
        host,
        selector: "github:jdx/mise@2026.10.14",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: "https://github.com/jdx/mise",
        source_commit: "d7131a7d69a2bdb2cfc84a66a5ff97329dd740e9",
        source_tree: "86511d143b7a9b3af5f2f43ed6cab6e60f03c72c",
        owner: "jdx/mise",
        version: "2026.10.14",
        abi: "mise-cli-v2026.10.14",
    }
}

const fn mbx(host: SourceBuildBootstrapHost) -> SourceBuildBootstrapAsset {
    let (asset_url, archive_sha256, binary_sha256) = match host {
        SourceBuildBootstrapHost::LinuxAmd64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.24.1/mbx-x86_64-unknown-linux-gnu.tar.gz",
            "d019fd3695d39549cd3eeb48b9518f0b43850d4d0bd9c49b955a8de9d30299fb",
            "3e646c39bf7ef50e09ad2e46ae7efc63ac65ce8c89dc01cb27a0307960ae4a33",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.24.1/mbx-aarch64-unknown-linux-gnu.tar.gz",
            "0699be8e61116ff6c8e45b95c031bf69e3781cccfee18072952d4efe81b35611",
            "f5ac3fe542fe3cafff598bbb474a7a03d55998d67463b17c394daa5e9287d3ce",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.24.1/mbx-aarch64-apple-darwin.tar.gz",
            "3ddb7d86b644275db140c9ae76f57bf72c5a10b7657f930466aad5ff7e025a85",
            "5e4a52b4480bdefb663b6937250a1136012238a8c1452103e0dabefdd7f222a6",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mbx,
        host,
        selector: "github:jdx/mr-boxington@1.24.1",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: SourceBuildBootstrapFormat::TarGzip,
        binary_member: "mbx",
        source_repository: "https://github.com/jdx/mr-boxington",
        source_commit: "ebb2b61b193d9ea487b67152d459e8332e8cf87e",
        source_tree: "9a416015df0ce5db6fc05b41e20de65f75f0915d",
        owner: "jdx/mr-boxington",
        version: "1.24.1",
        abi: "mbx-cli-v1.24.1",
    }
}

#[cfg(test)]
#[path = "catalog_source_build_bootstrap_tests.rs"]
mod tests;

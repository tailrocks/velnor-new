//! Closed official source-builder bootstrap assets.
//!
//! This authority owns only upstream bootstrap bytes used to acquire the
//! source-build path. It is separate from runtime distribution qualification:
//! an asset here does not establish Velnor-owned behavior or installation.
//!
//! Mise evidence was captured 2026-10-13 from release `v2026.10.13`; the
//! official source commit is `580a418ed8ce379669a1a98e9aa405aa6a860996` with
//! tree `bc9af7a05b0e29cab8d2f3ecba106db8aba65e05`. MBX evidence was captured
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
            "https://github.com/jdx/mise/releases/download/v2026.10.13/mise-v2026.10.13-linux-x64",
            "94c45523c7b6760b7da704f16051171ca32e8447a849a02266a439a4455e9a12",
            "94c45523c7b6760b7da704f16051171ca32e8447a849a02266a439a4455e9a12",
            SourceBuildBootstrapFormat::Binary,
            "",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.13/mise-v2026.10.13-linux-arm64.tar.gz",
            "ec1630fd441434a1b8c8c0074a53e3e8cc955ad313e9f354c44775eb19103a57",
            "983fabf36625566f216400b907b33b4daca99adb5bc3d3cdab123b5e791575b2",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.13/mise-v2026.10.13-macos-arm64.tar.gz",
            "b91f1b30bd9e7e6899899f0ff5ac5625a90c86daa652524f7b8922db3287ff60",
            "92fd6898dc213808d1c81f0bead9668dbea036e5946c94c0e82554603205a3aa",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mise,
        host,
        selector: "github:jdx/mise@2026.10.13",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: "https://github.com/jdx/mise",
        source_commit: "580a418ed8ce379669a1a98e9aa405aa6a860996",
        source_tree: "bc9af7a05b0e29cab8d2f3ecba106db8aba65e05",
        owner: "jdx/mise",
        version: "2026.10.13",
        abi: "mise-cli-v2026.10.13",
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

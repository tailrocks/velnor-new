// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::MISE_BINARY_SHA256_LINUX_X64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBuildBootstrapTool {
    Mise,
    Mbx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBuildBootstrapHost {
    LinuxAmd64,
    LinuxArm64,
    MacosArm64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBuildBootstrapFormat {
    Binary,
    TarGzip,
}

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
    #[must_use]
    pub const fn selector(&self) -> &'static str {
        self.selector
    }

    #[must_use]
    pub const fn asset_url(&self) -> &'static str {
        self.asset_url
    }

    #[must_use]
    pub const fn archive_sha256(&self) -> &'static str {
        self.archive_sha256
    }

    #[must_use]
    pub const fn binary_sha256(&self) -> &'static str {
        self.binary_sha256
    }

    #[must_use]
    pub const fn asset_format(&self) -> SourceBuildBootstrapFormat {
        self.asset_format
    }

    #[must_use]
    pub const fn binary_member(&self) -> &'static str {
        self.binary_member
    }

    #[must_use]
    pub const fn source_repository(&self) -> &'static str {
        self.source_repository
    }

    #[must_use]
    pub const fn source_commit(&self) -> &'static str {
        self.source_commit
    }

    #[must_use]
    pub const fn source_tree(&self) -> &'static str {
        self.source_tree
    }

    #[must_use]
    pub const fn owner(&self) -> &'static str {
        self.owner
    }

    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }

    #[must_use]
    pub const fn abi(&self) -> &'static str {
        self.abi
    }

    #[must_use]
    pub const fn tool(&self) -> SourceBuildBootstrapTool {
        self.tool
    }

    #[must_use]
    pub const fn host(&self) -> SourceBuildBootstrapHost {
        self.host
    }
}

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
            "https://github.com/jdx/mise/releases/download/v2026.10.25/mise-v2026.10.25-linux-x64",
            MISE_BINARY_SHA256_LINUX_X64,
            MISE_BINARY_SHA256_LINUX_X64,
            SourceBuildBootstrapFormat::Binary,
            "",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.25/mise-v2026.10.25-linux-arm64.tar.gz",
            "a7d1e2add53441ff76e7e971e7a3fb15c117741836ce51c026f5fb7eebe7dbcf",
            "bb09e88b13164ca90691a5cc042726d079f0ba26ab55527344cb173c2e83c998",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.25/mise-v2026.10.25-macos-arm64.tar.gz",
            "774236e3247e48d78078c0bce79e5327d4e4479f1a94c8487827f178197ee39b",
            "8a3ef00765b009278197aba627ac00d6e9c63f6aeb04d4f4b2cfe0e59ddc2fd6",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mise,
        host,
        selector: "github:jdx/mise@2026.10.25",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: "https://github.com/jdx/mise",
        source_commit: "3c78bbb2d58c0b668fe8d5bebf4a8cb66194e1aa",
        source_tree: "210ff139381d56299eff55059066ce456fdea622",
        owner: "jdx/mise",
        version: "2026.10.25",
        abi: "mise-cli-v2026.10.25",
    }
}

const fn mbx(host: SourceBuildBootstrapHost) -> SourceBuildBootstrapAsset {
    let (asset_url, archive_sha256, binary_sha256) = match host {
        SourceBuildBootstrapHost::LinuxAmd64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.29.1/mbx-x86_64-unknown-linux-gnu.tar.gz",
            "ca95f28c77711845c0455839792ca3f3070bdf007979ab75bf7e10c03b07fe3f",
            "4b0c0fcfc9329a9d76fd6bdefc1ee34822580864f5f5ba2f0b47dcbbffce2eca",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.29.1/mbx-aarch64-unknown-linux-gnu.tar.gz",
            "92e5554096a9b0c083aaf3d461262d31e3f7a0aa3d38ed2972c62c2b9a953c41",
            "495e2a1cf1595f45d540bd09d8034ef322142356859dced99b0254d9e9028c0e",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.29.1/mbx-aarch64-apple-darwin.tar.gz",
            "2c19baafcda87ad948f071c571f57b2cf9eaeee2c971c5536b93ae3a036d5e75",
            "1c6e342a341e567fd752dea8b665c57c92ba2e65c0bfb13e7555c2051097ec88",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mbx,
        host,
        selector: "github:jdx/mr-boxington@1.29.1",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: SourceBuildBootstrapFormat::TarGzip,
        binary_member: "mbx",
        source_repository: "https://github.com/jdx/mr-boxington",
        source_commit: "b0b67184f01c1de02082296d093af547ecf4c2ab",
        source_tree: "44535809652e19ffa6c26231ba9f03e141189b97",
        owner: "jdx/mr-boxington",
        version: "1.29.1",
        abi: "mbx-cli-v1.29.1",
    }
}

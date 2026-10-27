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
            "https://github.com/jdx/mise/releases/download/v2026.10.21/mise-v2026.10.21-linux-x64",
            MISE_BINARY_SHA256_LINUX_X64,
            MISE_BINARY_SHA256_LINUX_X64,
            SourceBuildBootstrapFormat::Binary,
            "",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.21/mise-v2026.10.21-linux-arm64.tar.gz",
            "1c05e4d4baaf1b4a9bf929e9c39b3512deb937c8d4c1bd52927499624018834f",
            "a7c96627e56c075543fe1c1aafeb289d2588d6ddfd4d705fce4ae140c57e190c",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.21/mise-v2026.10.21-macos-arm64.tar.gz",
            "d67b6006cb1144131910ce80d6cf52134f55b9c7da55648bd717df36b3f238d6",
            "b20d30788a819fade398a1cb92aee56c383e3c34324162ec39fb8567dee79ba6",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mise,
        host,
        selector: "github:jdx/mise@2026.10.21",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: "https://github.com/jdx/mise",
        source_commit: "61eb04dbc806cb646821e342b7abbbdaf1581e84",
        source_tree: "65c7692f05c45170f6a42352a9ada4c0f667e48d",
        owner: "jdx/mise",
        version: "2026.10.21",
        abi: "mise-cli-v2026.10.21",
    }
}

const fn mbx(host: SourceBuildBootstrapHost) -> SourceBuildBootstrapAsset {
    let (asset_url, archive_sha256, binary_sha256) = match host {
        SourceBuildBootstrapHost::LinuxAmd64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.28.0/mbx-x86_64-unknown-linux-gnu.tar.gz",
            "50ce9360221d9274a73d70c9d2d5285dc2d8353a101de0845e3da5bb0083bcfb",
            "6d3e1afe2a91f83aeed762a576d0a2b3c990a82f7e3e45926e1cd4b922f0df1b",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.28.0/mbx-aarch64-unknown-linux-gnu.tar.gz",
            "9056d34520991f80c9f708d36008fcc732e500d735591a8649029d476cb566bd",
            "bc9d537789f16da1e37b9ca64fde107246a902f916b989fe3f84b0a4d4bbd2a5",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.28.0/mbx-aarch64-apple-darwin.tar.gz",
            "ac73ba0637a443ba9c43354008f6bcd5d9e8a84b703e46f23202025c1f849472",
            "068cf7b41d58b5dd630e56b4980650a4f57f5e3267abc6d16a614088f499cebf",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mbx,
        host,
        selector: "github:jdx/mr-boxington@1.28.0",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: SourceBuildBootstrapFormat::TarGzip,
        binary_member: "mbx",
        source_repository: "https://github.com/jdx/mr-boxington",
        source_commit: "44f50c329a83dedd012e1e49cf9f265f79226a83",
        source_tree: "ccfdb5978fb74a7bd854a0cd9eeafa635619c50f",
        owner: "jdx/mr-boxington",
        version: "1.28.0",
        abi: "mbx-cli-v1.28.0",
    }
}

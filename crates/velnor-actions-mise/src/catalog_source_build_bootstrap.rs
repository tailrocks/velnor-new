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
            "https://github.com/jdx/mise/releases/download/v2026.11.1/mise-v2026.11.1-linux-x64",
            MISE_BINARY_SHA256_LINUX_X64,
            MISE_BINARY_SHA256_LINUX_X64,
            SourceBuildBootstrapFormat::Binary,
            "",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.11.1/mise-v2026.11.1-linux-arm64.tar.gz",
            "6b54d9223f8bce04f9168ef482d9ee27f219c1ea11f4903569660049e5dec5aa",
            "ed5dc8d5d8b334684409ef7abad2ffe5c22cf8e311905ae906c3246cbe2c5442",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.11.1/mise-v2026.11.1-macos-arm64.tar.gz",
            "e1eaf20469e586f65849256a0be6a58dd7a3e93ae8db68bc74dd7af5ca0fe7f6",
            "9ca1005930fed90e0f4c30d9b13fe4530fe88d0549a71349716a3aab9051c627",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mise,
        host,
        selector: "github:jdx/mise@2026.11.1",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: "https://github.com/jdx/mise",
        source_commit: "0fa558048f88cfc1d3e9ad29f52b26a11debb1d9",
        source_tree: "865bec85a4afdf369ce9ab731db5c6e38fe64ac9",
        owner: "jdx/mise",
        version: "2026.11.1",
        abi: "mise-cli-v2026.11.1",
    }
}

const fn mbx(host: SourceBuildBootstrapHost) -> SourceBuildBootstrapAsset {
    let (asset_url, archive_sha256, binary_sha256) = match host {
        SourceBuildBootstrapHost::LinuxAmd64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.29.2/mbx-x86_64-unknown-linux-gnu.tar.gz",
            "73e135d3e9e8b8ddc938b42407179a239917b7f02d3fa5f601f6783e5c0d36d6",
            "03337a94062a76bf08df445959a3b732416c3931752602edb8f36b12474a9400",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.29.2/mbx-aarch64-unknown-linux-gnu.tar.gz",
            "1b6fbaebe0ea2c9b565c862a2b46e4756394ad530ce95b07333743104750a6fb",
            "31c0dd83d4c54d942b6e3d04edcf7064bb0536f6ce5a64806c130f17377d478a",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.29.2/mbx-aarch64-apple-darwin.tar.gz",
            "621ce235d8f241c5a3728ca03d34b9864f466f4e8b3307d8a6d7101a24a71a60",
            "c3f14e1e8564f0aae89e5157f5c0d06cf0e38f17072f170bd9364e2b7ef3781e",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mbx,
        host,
        selector: "github:jdx/mr-boxington@1.29.2",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: SourceBuildBootstrapFormat::TarGzip,
        binary_member: "mbx",
        source_repository: "https://github.com/jdx/mr-boxington",
        source_commit: "7fa83b28d239bd39d1079127ffd2f8b0f92df00a",
        source_tree: "f0bcb9892fd7597d0acd741fc76f6ea92ffad501",
        owner: "jdx/mr-boxington",
        version: "1.29.2",
        abi: "mbx-cli-v1.29.2",
    }
}

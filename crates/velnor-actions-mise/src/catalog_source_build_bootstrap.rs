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
            "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-linux-x64",
            "e97c38973336aa2ff350fea50f47b412b266fe263b102b7200ff1e9b118c6942",
            "e97c38973336aa2ff350fea50f47b412b266fe263b102b7200ff1e9b118c6942",
            SourceBuildBootstrapFormat::Binary,
            "",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-linux-arm64.tar.gz",
            "c5a49a47f8184f5649f3d8685b408bd72325a823f41cf618c60f3ffc303090a1",
            "3a3c72376687ba21a588d0f5a2c88c76f483fb6cdece7b0fc43d5e3132b08a52",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-macos-arm64.tar.gz",
            "1b1cd6ad4a6176db5ddaf03bf71e131b26324af09cfcf14d7d7038bee60be85d",
            "d40e2fcd96fed7c70b04af282c36534d87d0318bef89c16fa00c9f98e55ade88",
            SourceBuildBootstrapFormat::TarGzip,
            "mise/bin/mise",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mise,
        host,
        selector: "github:jdx/mise@2026.10.18",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format,
        binary_member,
        source_repository: "https://github.com/jdx/mise",
        source_commit: "8369bc78dc4761660337a2518b1445447438159d",
        source_tree: "f6484ac76553c3dca10c7ea690d01e359dac098b",
        owner: "jdx/mise",
        version: "2026.10.18",
        abi: "mise-cli-v2026.10.18",
    }
}

const fn mbx(host: SourceBuildBootstrapHost) -> SourceBuildBootstrapAsset {
    let (asset_url, archive_sha256, binary_sha256) = match host {
        SourceBuildBootstrapHost::LinuxAmd64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-x86_64-unknown-linux-gnu.tar.gz",
            "f530d0870efcf15c845314ffccd049f53eaec9a35e9f42c0dec0e16b3135e31c",
            "d02159722e826f1ed0729b07abaa5767907b7f5f9d3c2658f832e33da42c1907",
        ),
        SourceBuildBootstrapHost::LinuxArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-aarch64-unknown-linux-gnu.tar.gz",
            "8a1aaf0a495e455d2ea09430c92c59169eef6a29a140c6b21a6d1581fe252f86",
            "1b464d5b790b146d92cdef0a923cf00cc6d2a3b3d5acd69083b2bd75d604f1d4",
        ),
        SourceBuildBootstrapHost::MacosArm64 => (
            "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-aarch64-apple-darwin.tar.gz",
            "b23a6f67c4fe8c5ef5c3f5b7a747de69d5c9ffe4d00c642eb6e835736cd8dc22",
            "13f3409d70ebb1b76992f8d28c57d2954b9bc6eccb01eeb738af64b23b386421",
        ),
    };
    SourceBuildBootstrapAsset {
        tool: SourceBuildBootstrapTool::Mbx,
        host,
        selector: "github:jdx/mr-boxington@1.27.0",
        asset_url,
        archive_sha256,
        binary_sha256,
        asset_format: SourceBuildBootstrapFormat::TarGzip,
        binary_member: "mbx",
        source_repository: "https://github.com/jdx/mr-boxington",
        source_commit: "5a50200abf386580642b87eca2995156316b56a7",
        source_tree: "9d1061348a5724ab331264bf83eef9169878e94b",
        owner: "jdx/mr-boxington",
        version: "1.27.0",
        abi: "mbx-cli-v1.27.0",
    }
}

//! Exact six-row official bootstrap catalog proofs.

use super::*;

#[derive(Clone, Copy)]
struct Expected {
    tool: SourceBuildBootstrapTool,
    host: SourceBuildBootstrapHost,
    selector: &'static str,
    url: &'static str,
    archive: &'static str,
    binary: &'static str,
    format: SourceBuildBootstrapFormat,
    member: &'static str,
    repository: &'static str,
    commit: &'static str,
    tree: &'static str,
    owner: &'static str,
    version: &'static str,
    abi: &'static str,
}

const MEASURED_ROWS: [Expected; 6] = [
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxAmd64,
        selector: "github:jdx/mise@2026.10.18",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-linux-x64",
        archive: "e97c38973336aa2ff350fea50f47b412b266fe263b102b7200ff1e9b118c6942",
        binary: "e97c38973336aa2ff350fea50f47b412b266fe263b102b7200ff1e9b118c6942",
        format: SourceBuildBootstrapFormat::Binary,
        member: "",
        repository: "https://github.com/jdx/mise",
        commit: "8369bc78dc4761660337a2518b1445447438159d",
        tree: "f6484ac76553c3dca10c7ea690d01e359dac098b",
        owner: "jdx/mise",
        version: "2026.10.18",
        abi: "mise-cli-v2026.10.18",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mise@2026.10.18",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-linux-arm64.tar.gz",
        archive: "c5a49a47f8184f5649f3d8685b408bd72325a823f41cf618c60f3ffc303090a1",
        binary: "3a3c72376687ba21a588d0f5a2c88c76f483fb6cdece7b0fc43d5e3132b08a52",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "8369bc78dc4761660337a2518b1445447438159d",
        tree: "f6484ac76553c3dca10c7ea690d01e359dac098b",
        owner: "jdx/mise",
        version: "2026.10.18",
        abi: "mise-cli-v2026.10.18",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mise@2026.10.18",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-macos-arm64.tar.gz",
        archive: "1b1cd6ad4a6176db5ddaf03bf71e131b26324af09cfcf14d7d7038bee60be85d",
        binary: "d40e2fcd96fed7c70b04af282c36534d87d0318bef89c16fa00c9f98e55ade88",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "8369bc78dc4761660337a2518b1445447438159d",
        tree: "f6484ac76553c3dca10c7ea690d01e359dac098b",
        owner: "jdx/mise",
        version: "2026.10.18",
        abi: "mise-cli-v2026.10.18",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxAmd64,
        selector: "github:jdx/mr-boxington@1.27.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-x86_64-unknown-linux-gnu.tar.gz",
        archive: "f530d0870efcf15c845314ffccd049f53eaec9a35e9f42c0dec0e16b3135e31c",
        binary: "d02159722e826f1ed0729b07abaa5767907b7f5f9d3c2658f832e33da42c1907",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "5a50200abf386580642b87eca2995156316b56a7",
        tree: "9d1061348a5724ab331264bf83eef9169878e94b",
        owner: "jdx/mr-boxington",
        version: "1.27.0",
        abi: "mbx-cli-v1.27.0",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mr-boxington@1.27.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-aarch64-unknown-linux-gnu.tar.gz",
        archive: "8a1aaf0a495e455d2ea09430c92c59169eef6a29a140c6b21a6d1581fe252f86",
        binary: "1b464d5b790b146d92cdef0a923cf00cc6d2a3b3d5acd69083b2bd75d604f1d4",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "5a50200abf386580642b87eca2995156316b56a7",
        tree: "9d1061348a5724ab331264bf83eef9169878e94b",
        owner: "jdx/mr-boxington",
        version: "1.27.0",
        abi: "mbx-cli-v1.27.0",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mr-boxington@1.27.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-aarch64-apple-darwin.tar.gz",
        archive: "b23a6f67c4fe8c5ef5c3f5b7a747de69d5c9ffe4d00c642eb6e835736cd8dc22",
        binary: "13f3409d70ebb1b76992f8d28c57d2954b9bc6eccb01eeb738af64b23b386421",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "5a50200abf386580642b87eca2995156316b56a7",
        tree: "9d1061348a5724ab331264bf83eef9169878e94b",
        owner: "jdx/mr-boxington",
        version: "1.27.0",
        abi: "mbx-cli-v1.27.0",
    },
];

#[test]
fn every_official_row_has_two_64_character_lower_hex_hashes() {
    for tool in [
        SourceBuildBootstrapTool::Mise,
        SourceBuildBootstrapTool::Mbx,
    ] {
        for host in [
            SourceBuildBootstrapHost::LinuxAmd64,
            SourceBuildBootstrapHost::LinuxArm64,
            SourceBuildBootstrapHost::MacosArm64,
        ] {
            let asset = official(tool, host);
            for digest in [asset.archive_sha256(), asset.binary_sha256()] {
                assert_eq!(digest.len(), 64);
                assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
                assert_eq!(digest.to_ascii_lowercase(), digest);
                assert_ne!(digest, "0".repeat(64));
            }
        }
    }
}

#[test]
fn mise_linux_arm64_archive_uses_official_release_checksum() {
    // From v2026.10.18/SHASUMS256.txt, confirmed against fetched archive bytes.
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mise,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "c5a49a47f8184f5649f3d8685b408bd72325a823f41cf618c60f3ffc303090a1"
    );
}

#[test]
fn mbx_archives_use_official_v1270_release_checksums() {
    // Official v1.27.0 release metadata and SHA256SUMS agree with downloaded archive bytes.
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mbx,
            SourceBuildBootstrapHost::LinuxAmd64
        )
        .archive_sha256(),
        "f530d0870efcf15c845314ffccd049f53eaec9a35e9f42c0dec0e16b3135e31c"
    );
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mbx,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "8a1aaf0a495e455d2ea09430c92c59169eef6a29a140c6b21a6d1581fe252f86"
    );
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mbx,
            SourceBuildBootstrapHost::MacosArm64
        )
        .archive_sha256(),
        "b23a6f67c4fe8c5ef5c3f5b7a747de69d5c9ffe4d00c642eb6e835736cd8dc22"
    );
}

#[test]
fn official_rows_match_measured_six_row_catalog() {
    for row in MEASURED_ROWS {
        assert_asset(row);
    }
}

fn assert_asset(expected: Expected) {
    let actual = official(expected.tool, expected.host);
    assert_eq!(actual.tool(), expected.tool);
    assert_eq!(actual.host(), expected.host);
    assert_eq!(actual.selector(), expected.selector);
    assert_eq!(actual.asset_url(), expected.url);
    assert_eq!(actual.archive_sha256(), expected.archive);
    assert_eq!(actual.binary_sha256(), expected.binary);
    assert_eq!(actual.asset_format(), expected.format);
    assert_eq!(actual.binary_member(), expected.member);
    assert_eq!(actual.source_repository(), expected.repository);
    assert_eq!(actual.source_commit(), expected.commit);
    assert_eq!(actual.source_tree(), expected.tree);
    assert_eq!(actual.owner(), expected.owner);
    assert_eq!(actual.version(), expected.version);
    assert_eq!(actual.abi(), expected.abi);
}

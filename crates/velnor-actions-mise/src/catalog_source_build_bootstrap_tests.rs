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
        selector: "github:jdx/mise@2026.10.17",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.17/mise-v2026.10.17-linux-x64",
        archive: "fe6dd17e68e09682b31d2e7bb7bb755d22636ac1af5d94621e300f2ca9e9fe8a",
        binary: "fe6dd17e68e09682b31d2e7bb7bb755d22636ac1af5d94621e300f2ca9e9fe8a",
        format: SourceBuildBootstrapFormat::Binary,
        member: "",
        repository: "https://github.com/jdx/mise",
        commit: "f0636505badf917a68cf5d5ed815c046593eaed4",
        tree: "1913fc52b499ae4de354141ee43f922c80df6560",
        owner: "jdx/mise",
        version: "2026.10.17",
        abi: "mise-cli-v2026.10.17",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mise@2026.10.17",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.17/mise-v2026.10.17-linux-arm64.tar.gz",
        archive: "cac510e51793ad45e65fe92305bdbe4309c9c60b5907badbc7eba111738ab08f",
        binary: "e90904144ba4377c5fbe385f429830282c68f3b4f9c690bf6c5a2ae9b74a809b",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "f0636505badf917a68cf5d5ed815c046593eaed4",
        tree: "1913fc52b499ae4de354141ee43f922c80df6560",
        owner: "jdx/mise",
        version: "2026.10.17",
        abi: "mise-cli-v2026.10.17",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mise@2026.10.17",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.17/mise-v2026.10.17-macos-arm64.tar.gz",
        archive: "d7c50c1683524a764260986ccc9c41c0de5e1fcd491714b5640b657b76781af6",
        binary: "9e27709db5eb1e900d83149418d3067309f2a5004d4994d94246ab658ba4a3ce",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "f0636505badf917a68cf5d5ed815c046593eaed4",
        tree: "1913fc52b499ae4de354141ee43f922c80df6560",
        owner: "jdx/mise",
        version: "2026.10.17",
        abi: "mise-cli-v2026.10.17",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxAmd64,
        selector: "github:jdx/mr-boxington@1.26.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.26.0/mbx-x86_64-unknown-linux-gnu.tar.gz",
        archive: "decb2c1233d194d9d8f98fa926d86a5f726e9091e92fcff75f44a53ed9cab2b9",
        binary: "5e1df60044b6d386ef4bc03290e9109785fdc569911540203c14ce470a08c362",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "96429d5be1362e262bd334bf51c033c883719e8b",
        tree: "199840e87f77efa3a03b4f3203d9f834b728664c",
        owner: "jdx/mr-boxington",
        version: "1.26.0",
        abi: "mbx-cli-v1.26.0",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mr-boxington@1.26.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.26.0/mbx-aarch64-unknown-linux-gnu.tar.gz",
        archive: "30dad5b98974a37447249572c2d4f1dc065b3b774fa74f20f8a37bb73fb26767",
        binary: "1c8387b4ae32c8f454718676af93ec48945c9b5e631dedd1a51fb8d5c396d668",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "96429d5be1362e262bd334bf51c033c883719e8b",
        tree: "199840e87f77efa3a03b4f3203d9f834b728664c",
        owner: "jdx/mr-boxington",
        version: "1.26.0",
        abi: "mbx-cli-v1.26.0",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mr-boxington@1.26.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.26.0/mbx-aarch64-apple-darwin.tar.gz",
        archive: "5927803d54f313600f7c6792369867eb26f9dfaa909b04fb5727efcf0d17b0b7",
        binary: "8f4ab568569c5ade98482236d157ed4ad70879e270fcd6757ae494392697c388",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "96429d5be1362e262bd334bf51c033c883719e8b",
        tree: "199840e87f77efa3a03b4f3203d9f834b728664c",
        owner: "jdx/mr-boxington",
        version: "1.26.0",
        abi: "mbx-cli-v1.26.0",
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
    // From v2026.10.17/SHASUMS256.txt, confirmed against fetched archive bytes.
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mise,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "cac510e51793ad45e65fe92305bdbe4309c9c60b5907badbc7eba111738ab08f"
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

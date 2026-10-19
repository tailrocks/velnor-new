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
        selector: "github:jdx/mise@2026.10.14",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.14/mise-v2026.10.14-linux-x64",
        archive: "50be7185bcf75929f01cc5630276bcb43ca367f7c5b176e2580903d28c611dc4",
        binary: "50be7185bcf75929f01cc5630276bcb43ca367f7c5b176e2580903d28c611dc4",
        format: SourceBuildBootstrapFormat::Binary,
        member: "",
        repository: "https://github.com/jdx/mise",
        commit: "d7131a7d69a2bdb2cfc84a66a5ff97329dd740e9",
        tree: "86511d143b7a9b3af5f2f43ed6cab6e60f03c72c",
        owner: "jdx/mise",
        version: "2026.10.14",
        abi: "mise-cli-v2026.10.14",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mise@2026.10.14",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.14/mise-v2026.10.14-linux-arm64.tar.gz",
        archive: "ff76e1d99fc27bcb2b3b38d1afd0798a785088aad93cc4206f2227195d2063cd",
        binary: "a9a8eec284d1c8e1a76685dcfa2fd609e76773fe56546eb72e4568dd1f50c1c3",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "d7131a7d69a2bdb2cfc84a66a5ff97329dd740e9",
        tree: "86511d143b7a9b3af5f2f43ed6cab6e60f03c72c",
        owner: "jdx/mise",
        version: "2026.10.14",
        abi: "mise-cli-v2026.10.14",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mise@2026.10.14",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.14/mise-v2026.10.14-macos-arm64.tar.gz",
        archive: "eb68c824e16af90f61e70e0d785179f335b1b95466606a5e0d8b9da504bb7bb2",
        binary: "005b32b9c22ca9cb63ff9d35c74b3abe69c433e9ca655fea288ba3dcbe7e4918",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "d7131a7d69a2bdb2cfc84a66a5ff97329dd740e9",
        tree: "86511d143b7a9b3af5f2f43ed6cab6e60f03c72c",
        owner: "jdx/mise",
        version: "2026.10.14",
        abi: "mise-cli-v2026.10.14",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxAmd64,
        selector: "github:jdx/mr-boxington@1.24.1",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.24.1/mbx-x86_64-unknown-linux-gnu.tar.gz",
        archive: "d019fd3695d39549cd3eeb48b9518f0b43850d4d0bd9c49b955a8de9d30299fb",
        binary: "3e646c39bf7ef50e09ad2e46ae7efc63ac65ce8c89dc01cb27a0307960ae4a33",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "ebb2b61b193d9ea487b67152d459e8332e8cf87e",
        tree: "9a416015df0ce5db6fc05b41e20de65f75f0915d",
        owner: "jdx/mr-boxington",
        version: "1.24.1",
        abi: "mbx-cli-v1.24.1",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mr-boxington@1.24.1",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.24.1/mbx-aarch64-unknown-linux-gnu.tar.gz",
        archive: "0699be8e61116ff6c8e45b95c031bf69e3781cccfee18072952d4efe81b35611",
        binary: "f5ac3fe542fe3cafff598bbb474a7a03d55998d67463b17c394daa5e9287d3ce",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "ebb2b61b193d9ea487b67152d459e8332e8cf87e",
        tree: "9a416015df0ce5db6fc05b41e20de65f75f0915d",
        owner: "jdx/mr-boxington",
        version: "1.24.1",
        abi: "mbx-cli-v1.24.1",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mr-boxington@1.24.1",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.24.1/mbx-aarch64-apple-darwin.tar.gz",
        archive: "3ddb7d86b644275db140c9ae76f57bf72c5a10b7657f930466aad5ff7e025a85",
        binary: "5e4a52b4480bdefb663b6937250a1136012238a8c1452103e0dabefdd7f222a6",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "ebb2b61b193d9ea487b67152d459e8332e8cf87e",
        tree: "9a416015df0ce5db6fc05b41e20de65f75f0915d",
        owner: "jdx/mr-boxington",
        version: "1.24.1",
        abi: "mbx-cli-v1.24.1",
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
    // From v2026.10.14/SHASUMS256.txt, confirmed against fetched archive bytes.
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mise,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "ff76e1d99fc27bcb2b3b38d1afd0798a785088aad93cc4206f2227195d2063cd"
    );
}

#[test]
fn mbx_archives_use_official_v1241_release_checksums() {
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mbx,
            SourceBuildBootstrapHost::LinuxAmd64
        )
        .archive_sha256(),
        "d019fd3695d39549cd3eeb48b9518f0b43850d4d0bd9c49b955a8de9d30299fb"
    );
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mbx,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "0699be8e61116ff6c8e45b95c031bf69e3781cccfee18072952d4efe81b35611"
    );
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mbx,
            SourceBuildBootstrapHost::MacosArm64
        )
        .archive_sha256(),
        "3ddb7d86b644275db140c9ae76f57bf72c5a10b7657f930466aad5ff7e025a85"
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

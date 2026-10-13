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
        selector: "github:jdx/mise@2026.10.7",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.7/mise-v2026.10.7-linux-x64",
        archive: "6eb1b890e90818417ca34c90dbbd47881917d5cd199f31b63b062ea9c6b18d85",
        binary: "6eb1b890e90818417ca34c90dbbd47881917d5cd199f31b63b062ea9c6b18d85",
        format: SourceBuildBootstrapFormat::Binary,
        member: "",
        repository: "https://github.com/jdx/mise",
        commit: "4599c53b4286ff101de876f8122feec0797b48b2",
        tree: "d500ae4a7705b79c90358ec6f153a7c7db80abdc",
        owner: "jdx/mise",
        version: "2026.10.7",
        abi: "mise-cli-v2026.10.7",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mise@2026.10.7",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.7/mise-v2026.10.7-linux-arm64.tar.gz",
        archive: "67bfc43bcc28de3a461b29fcf13a94a06019e4a1d3be37e4ff2c1347a576449b",
        binary: "c7108d85a32ba17e4747d31d4a42f39f0c134f16211e204e8ef0a49d4f518fe1",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "4599c53b4286ff101de876f8122feec0797b48b2",
        tree: "d500ae4a7705b79c90358ec6f153a7c7db80abdc",
        owner: "jdx/mise",
        version: "2026.10.7",
        abi: "mise-cli-v2026.10.7",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mise@2026.10.7",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.7/mise-v2026.10.7-macos-arm64.tar.gz",
        archive: "5841e5ab5009b4c4dd2b641ddbfc6777cc1b9c9c0ffd375001e294540e9e9cc8",
        binary: "f5171e341518a57e8c4e9280e28443e35d66212c51164c83be76794e0a78b014",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "4599c53b4286ff101de876f8122feec0797b48b2",
        tree: "d500ae4a7705b79c90358ec6f153a7c7db80abdc",
        owner: "jdx/mise",
        version: "2026.10.7",
        abi: "mise-cli-v2026.10.7",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxAmd64,
        selector: "github:jdx/mr-boxington@1.21.1",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.21.1/mbx-x86_64-unknown-linux-gnu.tar.gz",
        archive: "1ecb4d55582a40a1227e8ca3450da054ea464e5ff976bb5762943fb1ce6f31da",
        binary: "97984b8c92953cefc027014d24c8abf8773f2d12ded0385156d2050da8d5fb8c",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313",
        tree: "1158c764f3893bacbd3a2f3e51990a9de1cb3712",
        owner: "jdx/mr-boxington",
        version: "1.21.1",
        abi: "mbx-cli-v1.21.1",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mr-boxington@1.21.1",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.21.1/mbx-aarch64-unknown-linux-gnu.tar.gz",
        archive: "a783ff78192a3cd299cfbf2b4b8a8dc16b8142c7bb962e9e3a027b64085189b8",
        binary: "e39ab5b1617c9ac72108058d899d931c8d2f553a0e6ba31b95509c8b79260df4",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313",
        tree: "1158c764f3893bacbd3a2f3e51990a9de1cb3712",
        owner: "jdx/mr-boxington",
        version: "1.21.1",
        abi: "mbx-cli-v1.21.1",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mr-boxington@1.21.1",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.21.1/mbx-aarch64-apple-darwin.tar.gz",
        archive: "99464a5bad96c3a472714faa4277aac22193ea9a385dd09892f5c1bbee9c56ba",
        binary: "ed67908a8661b84fea1ad41f70ed502b77cbcc704ea90918f16b2ce7045408dc",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313",
        tree: "1158c764f3893bacbd3a2f3e51990a9de1cb3712",
        owner: "jdx/mr-boxington",
        version: "1.21.1",
        abi: "mbx-cli-v1.21.1",
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

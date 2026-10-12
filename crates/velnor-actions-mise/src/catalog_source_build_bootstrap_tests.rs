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
        selector: "github:jdx/mise@2026.10.8",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.8/mise-v2026.10.8-linux-x64",
        archive: "c72477ae6c3a8e7d817a322f43da70e4643e75ee5c21265f1c19c894273b7428",
        binary: "c72477ae6c3a8e7d817a322f43da70e4643e75ee5c21265f1c19c894273b7428",
        format: SourceBuildBootstrapFormat::Binary,
        member: "",
        repository: "https://github.com/jdx/mise",
        commit: "b671d356aaf33b87e48e7469a77abb40c3553c68",
        tree: "ababca8f764247f1fc0c3bbccf66eaf16b56d27f",
        owner: "jdx/mise",
        version: "2026.10.8",
        abi: "mise-cli-v2026.10.8",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mise@2026.10.8",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.8/mise-v2026.10.8-linux-arm64.tar.gz",
        archive: "88aea77e41c2a3c45a9cbd64df9516ba887017442846213b3de178acbd3c9a46",
        binary: "6ee0bd8173f1347514f4974d55fb1fae13aa7f6de0a55338adbefa0667cad7e5",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "b671d356aaf33b87e48e7469a77abb40c3553c68",
        tree: "ababca8f764247f1fc0c3bbccf66eaf16b56d27f",
        owner: "jdx/mise",
        version: "2026.10.8",
        abi: "mise-cli-v2026.10.8",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mise@2026.10.8",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.8/mise-v2026.10.8-macos-arm64.tar.gz",
        archive: "8228699f316dc17931beb86f7a1aac70e4685535430441492ff4e857c72535ad",
        binary: "dd77de9a7574e38d8409fb06fa85e1581415896639d075987791d8c4e5a57b73",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "b671d356aaf33b87e48e7469a77abb40c3553c68",
        tree: "ababca8f764247f1fc0c3bbccf66eaf16b56d27f",
        owner: "jdx/mise",
        version: "2026.10.8",
        abi: "mise-cli-v2026.10.8",
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
fn mise_linux_arm64_archive_uses_official_release_checksum() {
    // From v2026.10.8/SHASUMS256.txt, confirmed against fetched archive bytes.
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mise,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "88aea77e41c2a3c45a9cbd64df9516ba887017442846213b3de178acbd3c9a46"
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

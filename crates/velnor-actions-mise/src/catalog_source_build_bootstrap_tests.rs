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
        selector: "github:jdx/mise@2026.10.9",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.9/mise-v2026.10.9-linux-x64",
        archive: "6ab6646b52b83f6bbd51c9d0d3ae54264ffa90efd1ac6d7c0ec5a86bbc6a0030",
        binary: "6ab6646b52b83f6bbd51c9d0d3ae54264ffa90efd1ac6d7c0ec5a86bbc6a0030",
        format: SourceBuildBootstrapFormat::Binary,
        member: "",
        repository: "https://github.com/jdx/mise",
        commit: "ac91d06fa6100935b7a07b08bebf03dd6dd9a0dd",
        tree: "8d37934c1dfeb12ec69abf171a3fbd187317ffd0",
        owner: "jdx/mise",
        version: "2026.10.9",
        abi: "mise-cli-v2026.10.9",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mise@2026.10.9",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.9/mise-v2026.10.9-linux-arm64.tar.gz",
        archive: "9a58b5b72e58ed66f38d6d2550c2f6279d8c39884574c389a39a9c441a5580b9",
        binary: "e5cf4820b0d3fd47776acf7811c855a52277262d35dcda1a9e8754e6d5442a39",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "ac91d06fa6100935b7a07b08bebf03dd6dd9a0dd",
        tree: "8d37934c1dfeb12ec69abf171a3fbd187317ffd0",
        owner: "jdx/mise",
        version: "2026.10.9",
        abi: "mise-cli-v2026.10.9",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mise@2026.10.9",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.9/mise-v2026.10.9-macos-arm64.tar.gz",
        archive: "2869b6680e4a7f75d30cb7971bbd66c62689eac0ce354327224700f6ea190437",
        binary: "93a0703c25fc15cd4fbe02019c9ea23ca9045e077af5e96c01207e1346edb14a",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "ac91d06fa6100935b7a07b08bebf03dd6dd9a0dd",
        tree: "8d37934c1dfeb12ec69abf171a3fbd187317ffd0",
        owner: "jdx/mise",
        version: "2026.10.9",
        abi: "mise-cli-v2026.10.9",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxAmd64,
        selector: "github:jdx/mr-boxington@1.24.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.24.0/mbx-x86_64-unknown-linux-gnu.tar.gz",
        archive: "3e35b12dd105875fa0505c45224eaf7e2fd7103d86199ffca97c0cd4093e708f",
        binary: "24a3f7a501b9a8a64fdbc9c2ec9e619fd97fc448c477edbef42aac16e4a10b51",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "1b62dc2c795c93630d4ce28c4d9d2540d6e776ea",
        tree: "a605270ae31ed6e7770fd6b2a13cf13861faceda",
        owner: "jdx/mr-boxington",
        version: "1.24.0",
        abi: "mbx-cli-v1.24.0",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mr-boxington@1.24.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.24.0/mbx-aarch64-unknown-linux-gnu.tar.gz",
        archive: "80a2909eff63aaf5b9901f923d68ab8b70056bb11d5fd025535614033e48dfcf",
        binary: "f4e20e4360afa87a4ef2b44d91393a865a16845833ea0f26a2b26ece53642fc6",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "1b62dc2c795c93630d4ce28c4d9d2540d6e776ea",
        tree: "a605270ae31ed6e7770fd6b2a13cf13861faceda",
        owner: "jdx/mr-boxington",
        version: "1.24.0",
        abi: "mbx-cli-v1.24.0",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mr-boxington@1.24.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.24.0/mbx-aarch64-apple-darwin.tar.gz",
        archive: "60c646b7ce5ae9cfdb67b1565a4ec81ec3a07290f1e7000a026a6db7fa247440",
        binary: "1946e8afb208a29fd5ad4f493f02d4d79ee630166120274b67c8bca177b53db6",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "1b62dc2c795c93630d4ce28c4d9d2540d6e776ea",
        tree: "a605270ae31ed6e7770fd6b2a13cf13861faceda",
        owner: "jdx/mr-boxington",
        version: "1.24.0",
        abi: "mbx-cli-v1.24.0",
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
    // From v2026.10.9/SHASUMS256.txt, confirmed against fetched archive bytes.
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mise,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "9a58b5b72e58ed66f38d6d2550c2f6279d8c39884574c389a39a9c441a5580b9"
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

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
        selector: "github:jdx/mise@2026.10.15",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.15/mise-v2026.10.15-linux-x64",
        archive: "e3b186d4c70627f27037be615becaf4acfb30e41fb6a1819486717dc94e4e2a9",
        binary: "e3b186d4c70627f27037be615becaf4acfb30e41fb6a1819486717dc94e4e2a9",
        format: SourceBuildBootstrapFormat::Binary,
        member: "",
        repository: "https://github.com/jdx/mise",
        commit: "597c1897dfed28d1c0dea9f98fba581059e42cd3",
        tree: "4786f2724d63cb59e06226bde8d3d58c09cb80dd",
        owner: "jdx/mise",
        version: "2026.10.15",
        abi: "mise-cli-v2026.10.15",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mise@2026.10.15",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.15/mise-v2026.10.15-linux-arm64.tar.gz",
        archive: "ae36e72ea72d337ab7b367edb0f7016fa1502aa0e36ccb705acad7902c60677b",
        binary: "3abbaac305a465ef038eb4ea07b961c037a8b639108005e43f8e2c00a8f67c11",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "597c1897dfed28d1c0dea9f98fba581059e42cd3",
        tree: "4786f2724d63cb59e06226bde8d3d58c09cb80dd",
        owner: "jdx/mise",
        version: "2026.10.15",
        abi: "mise-cli-v2026.10.15",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mise@2026.10.15",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.15/mise-v2026.10.15-macos-arm64.tar.gz",
        archive: "8a44a56f72323c777298a1d5abd88ffd21d0fca19fab1ccccd389a912374f69c",
        binary: "826c4833fa7437a63f6027b86bc10c8b61b1163dc793c03ecde92f68ad167890",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "597c1897dfed28d1c0dea9f98fba581059e42cd3",
        tree: "4786f2724d63cb59e06226bde8d3d58c09cb80dd",
        owner: "jdx/mise",
        version: "2026.10.15",
        abi: "mise-cli-v2026.10.15",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxAmd64,
        selector: "github:jdx/mr-boxington@1.25.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.25.0/mbx-x86_64-unknown-linux-gnu.tar.gz",
        archive: "2b4efb2ef340d38ed2d840d32b5535e45be7541224378ff353c7a83f85744195",
        binary: "48424ca46f5f3b984f0d8b0c2dcafaaaaf8dbde50464b6ecfcef1b324bfc251d",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "2ade2bc6b661f1b40a55cfa7083fa9a4efc9348d",
        tree: "54170d2d1f8fa99bbd7f1bbce328cd69562e6305",
        owner: "jdx/mr-boxington",
        version: "1.25.0",
        abi: "mbx-cli-v1.25.0",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mr-boxington@1.25.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.25.0/mbx-aarch64-unknown-linux-gnu.tar.gz",
        archive: "26adcf79262395e7fb355203afa7deda0f18fd790585f6e49e51994b9d32b55c",
        binary: "aef1baabde6a1eb6299fcf03c33e61a15538426aa7e82c9204e5ff7419ec1c3b",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "2ade2bc6b661f1b40a55cfa7083fa9a4efc9348d",
        tree: "54170d2d1f8fa99bbd7f1bbce328cd69562e6305",
        owner: "jdx/mr-boxington",
        version: "1.25.0",
        abi: "mbx-cli-v1.25.0",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mbx,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mr-boxington@1.25.0",
        url: "https://github.com/jdx/mr-boxington/releases/download/v1.25.0/mbx-aarch64-apple-darwin.tar.gz",
        archive: "47630813d15a001cfceceeeb1f35008421fb22b8ba132106067d217e000b1b68",
        binary: "2d957002c399d77670c7222fc4475c021adacfa0b63eb709af9437b63de08e2d",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mbx",
        repository: "https://github.com/jdx/mr-boxington",
        commit: "2ade2bc6b661f1b40a55cfa7083fa9a4efc9348d",
        tree: "54170d2d1f8fa99bbd7f1bbce328cd69562e6305",
        owner: "jdx/mr-boxington",
        version: "1.25.0",
        abi: "mbx-cli-v1.25.0",
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
    // From v2026.10.15/SHASUMS256.txt, confirmed against fetched archive bytes.
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mise,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "ae36e72ea72d337ab7b367edb0f7016fa1502aa0e36ccb705acad7902c60677b"
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

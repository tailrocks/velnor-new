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
        selector: "github:jdx/mise@2026.10.19",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.19/mise-v2026.10.19-linux-x64",
        archive: "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
        binary: "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
        format: SourceBuildBootstrapFormat::Binary,
        member: "",
        repository: "https://github.com/jdx/mise",
        commit: "860cdc0969db81b1a076b1eb4574bd4b49bdf4e0",
        tree: "2fa25d5f04874ce37e374e25681fb35f4e9e5169",
        owner: "jdx/mise",
        version: "2026.10.19",
        abi: "mise-cli-v2026.10.19",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::LinuxArm64,
        selector: "github:jdx/mise@2026.10.19",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.19/mise-v2026.10.19-linux-arm64.tar.gz",
        archive: "f6dbd376928095d3bc8fde72eb1b27a8450d35c1795aa80c05aacf1ce42584aa",
        binary: "4a4ca53b28e5433c527f4f73fac1f98f992a17cf32ca136a58e2e375f790f627",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "860cdc0969db81b1a076b1eb4574bd4b49bdf4e0",
        tree: "2fa25d5f04874ce37e374e25681fb35f4e9e5169",
        owner: "jdx/mise",
        version: "2026.10.19",
        abi: "mise-cli-v2026.10.19",
    },
    Expected {
        tool: SourceBuildBootstrapTool::Mise,
        host: SourceBuildBootstrapHost::MacosArm64,
        selector: "github:jdx/mise@2026.10.19",
        url: "https://github.com/jdx/mise/releases/download/v2026.10.19/mise-v2026.10.19-macos-arm64.tar.gz",
        archive: "a8a581e115f4ebdbe2931c237949dbac7624bd8b4024bf3eb9da7981bde490c9",
        binary: "7dc61dfa71abbcda1c40d18440bc556b5d58b80e6042bc4f95fd106fa1bb3c66",
        format: SourceBuildBootstrapFormat::TarGzip,
        member: "mise/bin/mise",
        repository: "https://github.com/jdx/mise",
        commit: "860cdc0969db81b1a076b1eb4574bd4b49bdf4e0",
        tree: "2fa25d5f04874ce37e374e25681fb35f4e9e5169",
        owner: "jdx/mise",
        version: "2026.10.19",
        abi: "mise-cli-v2026.10.19",
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
    // From signed v2026.10.19/SHASUMS256.txt; extracted member also matches.
    assert_eq!(
        official(
            SourceBuildBootstrapTool::Mise,
            SourceBuildBootstrapHost::LinuxArm64
        )
        .archive_sha256(),
        "f6dbd376928095d3bc8fde72eb1b27a8450d35c1795aa80c05aacf1ce42584aa"
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

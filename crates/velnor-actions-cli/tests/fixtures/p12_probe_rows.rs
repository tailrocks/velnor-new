//! Canned source evidence for offline freshness probes.

const TOOL_PROBE_ROWS: &[(&str, &str, &str)] = &[
    (
        "https://api.github.com/repos/jdx/mise/releases/latest",
        "mise.json",
        "{\"tag_name\": \"v2026.10.0\"}",
    ),
    (
        "https://static.rust-lang.org/dist/channel-rust-stable.toml",
        "rust.toml",
        "[pkg.rust]\nversion = \"1.98.1 (48a229cea 2026-09-01)\"\n",
    ),
    (
        "https://api.github.com/repos/jdx/mr-boxington/releases/latest",
        "mbx.json",
        "[{\"tag_name\": \"v1.20.0-beta\", \"prerelease\": true}, {\"tag_name\": \"v1.19.0\"}]",
    ),
    (
        "https://api.github.com/repos/cli/cli/releases/latest",
        "gh.json",
        "{\"tag_name\": \"v2.101.0\"}",
    ),
    (
        "https://api.github.com/repos/rhysd/actionlint/releases/latest",
        "actionlint.json",
        "{\"tag_name\": \"v1.7.12\"}",
    ),
    (
        "https://api.github.com/repos/koalaman/shellcheck/releases/latest",
        "shellcheck.json",
        "{\"tag_name\": \"v0.11.0\"}",
    ),
    (
        "https://api.github.com/repos/zizmorcore/zizmor/releases/latest",
        "zizmor.json",
        "{\"tag_name\": \"v1.30.1\"}",
    ),
    (
        "https://crates.io/api/v1/crates/cargo-nextest",
        "nextest.json",
        "{\"crate\": {\"max_version\": \"0.9.146\"}}",
    ),
    (
        "https://api.github.com/repos/opentofu/opentofu/releases/latest",
        "opentofu.json",
        "{\"tag_name\": \"v1.13.1\"}",
    ),
    (
        "https://crates.io/api/v1/crates/release-plz",
        "release-plz.json",
        "{\"crate\": {\"max_version\": \"0.3.169\"}}",
    ),
    (
        "https://api.github.com/repos/oven-sh/bun/releases/latest",
        "bun.json",
        "{\"tag_name\": \"bun-v1.4.2\"}",
    ),
    (
        "https://api.github.com/repos/swiftlang/swift/releases/latest",
        "swift.json",
        "{\"tag_name\": \"swift-6.4.0-RELEASE\"}",
    ),
    (
        "https://api.github.com/repos/ruby/ruby/releases/latest",
        "ruby.json",
        "{\"tag_name\": \"v4.0.7\"}",
    ),
    (
        "https://api.github.com/repos/fsfe/reuse-tool/releases/latest",
        "reuse.json",
        "{\"tag_name\": \"v6.2.0\"}",
    ),
    (
        "https://api.github.com/repos/graalvm/graalvm-ce-builds/releases?per_page=10",
        "java.json",
        r#"[{"tag_name":"graal-25.4.4.1.1","html_url":"https://github.com/graalvm/graalvm-ce-builds/releases/tag/graal-25.4.4.1.1","draft":false,"prerelease":false,"assets":[{"name":"graalvm-community-jdk-25i4-25.0.4.1.1_linux-aarch64_bin.tar.gz","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","browser_download_url":"https://github.com/graalvm/graalvm-ce-builds/releases/download/graal-25.4.4.1.1/graalvm-community-jdk-25i4-25.0.4.1.1_linux-aarch64_bin.tar.gz"},{"name":"graalvm-community-jdk-25i4-25.0.4.1.1_linux-x64_bin.tar.gz","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","browser_download_url":"https://github.com/graalvm/graalvm-ce-builds/releases/download/graal-25.4.4.1.1/graalvm-community-jdk-25i4-25.0.4.1.1_linux-x64_bin.tar.gz"},{"name":"graalvm-community-jdk-25i4-25.0.4.1.1_macos-aarch64_bin.tar.gz","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","browser_download_url":"https://github.com/graalvm/graalvm-ce-builds/releases/download/graal-25.4.4.1.1/graalvm-community-jdk-25i4-25.0.4.1.1_macos-aarch64_bin.tar.gz"}]}]"#,
    ),
    (
        "https://api.github.com/repos/gradle/gradle/releases/latest",
        "gradle.json",
        "{\"tag_name\": \"v9.8.0\"}",
    ),
    (
        "https://www.python.org/downloads/",
        "python.html",
        "<a href=\"/downloads/release/python-3148/\">Download Python 3.14.8</a>",
    ),
    (
        "https://api.github.com/repos/astral-sh/uv/releases/latest",
        "uv.json",
        "{\"tag_name\": \"0.12.22\"}",
    ),
    (
        "https://crates.io/api/v1/crates/cargo-audit",
        "cargo-audit.json",
        "{\"crate\": {\"max_version\": \"0.22.2\"}}",
    ),
    (
        "https://api.github.com/repos/obi1kenobi/cargo-semver-checks/releases/latest",
        "cargo-semver-checks.json",
        "{\"tag_name\": \"v0.50.0\"}",
    ),
    (
        "https://crates.io/api/v1/crates/cargo-deny",
        "cargo-deny.json",
        "{\"crate\": {\"max_version\": \"0.20.2\"}}",
    ),
    (
        "https://api.github.com/repos/docker/buildx/releases/latest",
        "buildx.json",
        "{\"tag_name\": \"v0.37.2\"}",
    ),
    (
        "https://api.github.com/repos/moby/buildkit/releases/latest",
        "buildkit.json",
        "{\"tag_name\": \"v0.33.1\"}",
    ),
    (
        "https://api.github.com/repos/docker/buildkit-syft-scanner/releases/latest",
        "sbom-scanner.json",
        "{\"tag_name\": \"v1.12.0\"}",
    ),
    (
        "https://nodejs.org/dist/index.json",
        "node.json",
        "[{\"version\": \"v26.99.0\", \"lts\": false}, {\"version\": \"v24.21.0\", \"lts\": \"Krypton\"}, {\"version\": \"v24.99.0\", \"lts\": false}, {\"version\": \"v22.999.0\", \"lts\": \"Jod\"}]",
    ),
    (
        "https://api.github.com/repos/boltffi/boltffi/releases/latest",
        "boltffi.json",
        "{\"tag_name\": \"v0.30.1\"}",
    ),
    (
        "https://api.github.com/repos/yonaskolb/XcodeGen/releases/latest",
        "xcodegen.json",
        "{\"tag_name\": \"2.46.0\"}",
    ),
    (
        "https://api.github.com/repos/jqlang/jq/releases/latest",
        "jq.json",
        "{\"tag_name\": \"jq-1.8.2\"}",
    ),
    (
        "https://static.rust-lang.org/dist/channel-rust-1.99.0.toml",
        "rust-desktop.toml",
        "[pkg.rust]\nversion = \"1.99.0 (fixture static compiler role)\"\n",
    ),
    (
        "https://api.github.com/repos/realm/SwiftLint/releases/latest",
        "swiftlint.json",
        "{\"tag_name\": \"0.65.1\"}",
    ),
    (
        "https://api.github.com/repos/peripheryapp/periphery/releases/latest",
        "periphery.json",
        "{\"tag_name\": \"3.8.0\"}",
    ),
];

const ACTION_PROBE_ROWS: &[(&str, &str, &str)] = &[
    (
        "https://api.github.com/repos/Swatinem/rust-cache/tags",
        "rust-cache.json",
        "[{\"name\": \"v2.9.2\"}]",
    ),
    (
        "https://api.github.com/repos/jdx/mise-action/releases/latest",
        "mise-action.json",
        "{\"tag_name\": \"v4.3.0\"}",
    ),
    (
        "https://api.github.com/repos/actions/checkout/releases/latest",
        "checkout.json",
        "{\"tag_name\": \"v7.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/download-artifact/releases/latest",
        "download.json",
        "{\"tag_name\": \"v8.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/upload-artifact/releases/latest",
        "upload.json",
        "{\"tag_name\": \"v7.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/cache/releases/latest",
        "cache.json",
        "{\"tag_name\": \"v6.1.0\"}",
    ),
    (
        "https://api.github.com/repos/jdx/mr-boxington-action/releases",
        "mbx-action.json",
        "[{\"tag_name\": \"v1.6.0\"}]",
    ),
    (
        "https://api.github.com/repos/asamarts/alint/releases/latest",
        "alint.json",
        "{\"tag_name\": \"v0.17.0\"}",
    ),
    (
        "https://api.github.com/repos/docker/setup-buildx-action/releases/latest",
        "setup-buildx-action.json",
        "{\"tag_name\": \"v4.4.1\"}",
    ),
    (
        "https://api.github.com/repos/docker/login-action/releases/latest",
        "login-action.json",
        "{\"tag_name\": \"v4.6.0\"}",
    ),
    (
        "https://api.github.com/repos/docker/build-push-action/releases/latest",
        "build-push-action.json",
        "{\"tag_name\": \"v7.4.0\"}",
    ),
    (
        "https://api.github.com/repos/actions/configure-pages/releases/latest",
        "configure-pages.json",
        "{\"tag_name\": \"v6.0.0\"}",
    ),
    (
        "https://api.github.com/repos/actions/upload-pages-artifact/releases/latest",
        "upload-pages-artifact.json",
        "{\"tag_name\": \"v5.0.0\"}",
    ),
    (
        "https://api.github.com/repos/actions/deploy-pages/releases/latest",
        "deploy-pages.json",
        "{\"tag_name\": \"v5.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/attest/releases/latest",
        "attest.json",
        "{\"tag_name\": \"v4.2.2\"}",
    ),
];

/// (inventory source URL, canned file, canned body) for every probe row.
pub(crate) fn probe_rows() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut rows = TOOL_PROBE_ROWS.to_vec();
    rows.extend_from_slice(ACTION_PROBE_ROWS);
    rows
}

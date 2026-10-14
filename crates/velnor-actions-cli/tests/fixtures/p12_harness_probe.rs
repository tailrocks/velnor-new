//! Canned upstream probe rows for the freshness fixture.

use std::error::Error;

use super::{Fixture, write};

const TOOL_PROBE_ROWS: &[(&str, &str, &str)] = &[
    (
        "https://pypi.org/pypi/reuse/json",
        "reuse.json",
        "{\"info\":{\"version\":\"6.2.0\"}}",
    ),
    (
        "https://www.python.org/downloads/",
        "python.html",
        "<a>Download Python 3.14.8</a>",
    ),
    (
        "https://api.github.com/repos/astral-sh/uv/releases/latest",
        "uv.json",
        "{\"tag_name\":\"0.12.22\"}",
    ),
    (
        "https://api.github.com/repos/jdx/mise/releases/latest",
        "mise.json",
        "{\"tag_name\": \"v2026.10.4\"}",
    ),
    (
        "https://static.rust-lang.org/dist/channel-rust-stable.toml",
        "rust.toml",
        "[pkg.rust]\nversion = \"1.99.0 (abc 2026-10-01)\"\n",
    ),
    (
        "https://api.github.com/repos/jdx/mr-boxington/releases/latest",
        "mbx.json",
        "[{\"tag_name\": \"v1.20.0-beta\", \"prerelease\": true}, {\"tag_name\": \"v1.23.0\"}]",
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
        "{\"crate\": {\"max_version\": \"0.9.148\"}}",
    ),
    (
        "https://crates.io/api/v1/crates/release-plz",
        "release-plz.json",
        "{\"crate\": {\"max_version\": \"0.3.172\"}}",
    ),
    (
        "https://crates.io/api/v1/crates/release-plz",
        "release-plz.json",
        "{\"crate\": {\"max_version\": \"0.3.169\"}}",
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
];

const ACTION_PROBE_ROWS: &[(&str, &str, &str)] = &[
    (
        "https://api.github.com/repos/jdx/mise-action/releases/latest",
        "mise-action.json",
        "{\"tag_name\": \"v5.1.2\"}",
    ),
    (
        "https://api.github.com/repos/actions/checkout/commits/v7.0.1",
        "checkout-tag.json",
        "{\"sha\": \"3d3c42e5aac5ba805825da76410c181273ba90b1\"}",
    ),
    (
        "https://api.github.com/repos/actions/checkout/releases/latest",
        "checkout.json",
        "{\"tag_name\": \"v7.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/download-artifact/commits/v8.0.1",
        "download-tag.json",
        "{\"sha\": \"3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c\"}",
    ),
    (
        "https://api.github.com/repos/actions/download-artifact/releases/latest",
        "download.json",
        "{\"tag_name\": \"v8.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/upload-artifact/commits/v7.0.1",
        "upload-tag.json",
        "{\"sha\": \"043fb46d1a93c77aae656e7c1c64a875d1fc6a0a\"}",
    ),
    (
        "https://api.github.com/repos/actions/upload-artifact/releases/latest",
        "upload.json",
        "{\"tag_name\": \"v7.0.1\"}",
    ),
    (
        "https://api.github.com/repos/actions/cache/commits/v6.1.0",
        "cache-tag.json",
        "{\"sha\": \"55cc8345863c7cc4c66a329aec7e433d2d1c52a9\"}",
    ),
    (
        "https://api.github.com/repos/actions/cache/releases/latest",
        "cache.json",
        "{\"tag_name\": \"v6.1.0\"}",
    ),
    (
        "https://api.github.com/repos/jdx/mr-boxington-action/commits/v1.5.0",
        "mbx-action-tag.json",
        "{\"sha\": \"9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3\"}",
    ),
    (
        "https://api.github.com/repos/jdx/mr-boxington-action/releases",
        "mbx-action.json",
        "[{\"tag_name\": \"v1.5.0\"}]",
    ),
    (
        "https://api.github.com/repos/asamarts/alint/commits/v0.16.1",
        "alint-tag.json",
        "{\"sha\": \"9f9d34ba0eae3888299b9e570f43338b0e7f2cdb\"}",
    ),
    (
        "https://api.github.com/repos/asamarts/alint/releases/latest",
        "alint.json",
        "{\"tag_name\": \"v0.18.1\"}",
    ),
    (
        "https://api.github.com/repos/aws-actions/configure-aws-credentials/releases/latest",
        "aws-credentials.json",
        "{\"tag_name\": \"v6.3.0\"}",
    ),
    (
        "https://api.github.com/repos/Swatinem/rust-cache/commits/v2.9.2",
        "rust-cache-tag.json",
        "{\"sha\": \"6323deb102c322ba6fcbdcafc7e3dddab59af2b6\"}",
    ),
    (
        "https://api.github.com/repos/Swatinem/rust-cache/releases/latest",
        "rust-cache.json",
        "{\"tag_name\": \"v2.9.2\"}",
    ),
];

/// (inventory source URL, canned file, canned body) for every probe row.
pub(crate) fn probe_rows() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut rows = TOOL_PROBE_ROWS.to_vec();
    rows.extend_from_slice(ACTION_PROBE_ROWS);
    rows
}

/// Write canonical URL -> local file mappings for bounded upstream tests.
pub(crate) fn write_probe_fixture(
    fixture: &Fixture,
    include_bodies: bool,
) -> Result<(), Box<dyn Error>> {
    let inventory = std::fs::read_to_string(fixture.dir.join(".velnor/freshness-inventory.json"))?;
    let mut sources = String::new();
    let mut seen = std::collections::BTreeSet::new();
    for (url, file, body) in probe_rows() {
        assert!(inventory.contains(url), "inventory source {url}");
        assert!(seen.insert(url), "duplicate fixture URL {url}");
        sources.push_str(url);
        sources.push('\t');
        sources.push_str(file);
        sources.push('\n');
        if include_bodies {
            write(fixture.dir.as_path(), &format!("upstream/{file}"), body)?;
        }
    }
    write(fixture.dir.as_path(), "upstream/sources.tsv", &sources)
}

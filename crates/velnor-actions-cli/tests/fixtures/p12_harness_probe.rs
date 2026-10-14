//! Canned upstream probe rows for the freshness fixture.

use std::error::Error;

use super::{Fixture, write};

const TOOL_PROBE_ROWS: &[(&str, &str, &str)] = &[
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
        "https://api.github.com/repos/opentofu/opentofu/releases/latest",
        "opentofu.json",
        "{\"tag_name\": \"v1.13.1\"}",
    ),
];

const ACTION_PROBE_ROWS: &[(&str, &str, &str)] = &[
    (
        "https://api.github.com/repos/jdx/mise-action/releases/latest",
        "mise-action.json",
        "{\"tag_name\": \"v5.1.2\"}",
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
        "[{\"tag_name\": \"v1.5.0\"}]",
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
    write(fixture.dir.as_path(), "upstream/sources.tsv", &sources)?;
    if include_bodies {
        rewrite_inventory_sources(fixture)?;
    }
    Ok(())
}

/// Point inventoried sources at the canned files so the probe stays offline.
fn rewrite_inventory_sources(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let path = fixture.dir.join(".velnor/freshness-inventory.json");
    let mut inventory: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
    let root = fixture.dir.canonicalize()?;
    for section in ["tools", "actions"] {
        let Some(rows) = inventory
            .get_mut(section)
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        for row in rows {
            let Some(source) = row.get("source").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let Some((_, file, _)) = probe_rows().into_iter().find(|(url, _, _)| *url == source)
            else {
                continue;
            };
            let absolute = root.join("upstream").join(file);
            let fields = row
                .as_object_mut()
                .ok_or_else(|| std::io::Error::other("inventory row is not an object"))?;
            fields.insert(
                "source".to_owned(),
                serde_json::Value::String(format!("file://{}", absolute.display())),
            );
        }
    }
    std::fs::write(path, serde_json::to_vec(&inventory)?)?;
    Ok(())
}

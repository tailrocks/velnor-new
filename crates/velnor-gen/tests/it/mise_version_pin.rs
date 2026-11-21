// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

fn repo_file(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| unreachable!("cannot read {}: {e}", path.display()))
}

fn min_version(config: &str) -> String {
    let table: toml::Table = toml::from_str(config).unwrap_or_else(|e| unreachable!("{e}"));
    let Some(min) = table.get("min_version").and_then(toml::Value::as_str) else {
        unreachable!("mise.toml sets a string min_version")
    };
    min.to_owned()
}

#[test]
fn mise_version_pin_matches_min_version_in_mise_toml() {
    let pin = repo_file(".mise-version");
    let min = min_version(&repo_file("mise.toml"));
    assert_eq!(
        pin.trim(),
        min,
        ".mise-version pins Mise {} but mise.toml min_version is {min}; set both to the same release",
        pin.trim()
    );
}

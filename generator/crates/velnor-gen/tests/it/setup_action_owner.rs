// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

const ACTION: &str = "actions/setup-velnor/action.yml";
const GENERATOR_PACKAGES: [&str; 5] = [
    "velnor-gen",
    "velnor-model",
    "velnor-release",
    "velnor-render",
    "velnor-rust",
];

#[test]
fn the_setup_velnor_action_file_is_owned_by_the_generator_packages_alone()
-> Result<(), Box<dyn Error>> {
    let config: toml::Table = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.velnor/config.toml"),
    )?
    .parse()?;
    let rules = config
        .get("stacks")
        .and_then(|stacks| stacks.get("rust"))
        .and_then(|rust| rust.get("inputs"))
        .and_then(toml::Value::as_array)
        .ok_or("no [[stacks.rust.inputs]]")?;
    let strings = |rule: &toml::Value, key: &str| -> BTreeSet<String> {
        rule.get(key)
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(toml::Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let owners: Vec<BTreeSet<String>> = rules
        .iter()
        .filter(|rule| strings(rule, "globs").contains(ACTION))
        .map(|rule| strings(rule, "packages"))
        .collect();
    let want: BTreeSet<String> = GENERATOR_PACKAGES.map(str::to_owned).into();
    assert_eq!(owners, [want]);
    Ok(())
}

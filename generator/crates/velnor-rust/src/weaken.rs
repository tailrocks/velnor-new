// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use toml::Value;
use velnor_model::{ChangeSet, PathGlob, RepoFacts, RepoPath, Weakening};

use crate::config::{CONFIG_PATH, parse};
use crate::mise::MISE_CONFIGS;
use crate::roots::is_cargo_config;
use crate::toml_file::table;

const NEXTEST_CONFIG: &str = ".config/nextest.toml";
const MANIFEST: &str = "Cargo.toml";
const COMPILER_KEYS: [&str; 4] = [
    "rustc-wrapper",
    "rustc-workspace-wrapper",
    "rustc",
    "rustdoc",
];

pub(crate) fn is_nextest_config(path: &RepoPath) -> bool {
    named(path, NEXTEST_CONFIG)
}

fn named(path: &RepoPath, name: &str) -> bool {
    path.as_str() == name
        || path
            .as_str()
            .strip_suffix(name)
            .is_some_and(|rest| rest.ends_with('/'))
}

pub(crate) fn weakenings(files: &BTreeMap<RepoPath, Vec<u8>>) -> Vec<Weakening> {
    let mut found: Vec<Weakening> = files
        .iter()
        .flat_map(|(file, bytes)| in_file(file, bytes))
        .collect();
    found.sort();
    found
}

#[must_use]
pub fn introduced(facts: &RepoFacts, changes: &ChangeSet) -> Vec<Weakening> {
    let before = weakenings(
        &changes
            .base_files
            .iter()
            .map(|(path, text)| (path.clone(), text.clone().into_bytes()))
            .collect(),
    );
    facts
        .weakenings
        .iter()
        .filter(|weakening| changes.paths.contains(&weakening.file) && !before.contains(weakening))
        .cloned()
        .collect()
}

fn in_file(file: &RepoPath, bytes: &[u8]) -> Vec<Weakening> {
    let make = |setting: String, effect: String| Weakening {
        file: file.clone(),
        setting,
        effect,
    };
    if *file == CONFIG_PATH {
        return parse(bytes).map_or_else(
            |_| Vec::new(),
            |config| {
                let globs =
                    |key: &str, globs: &[PathGlob], effect: &dyn Fn(&PathGlob) -> String| {
                        globs
                            .iter()
                            .map(|glob| make(format!("{key} = \"{glob}\""), effect(glob)))
                            .collect::<Vec<_>>()
                    };
                let mut found = globs("exclude", &config.exclude, &|glob| {
                    format!("leaves {glob} out of the checks")
                });
                found.extend(config.inputs.iter().map(|rule| {
                    let packages = rule
                        .packages
                        .iter()
                        .map(|package| format!("\"{package}\""))
                        .collect::<Vec<_>>();
                    make(
                        format!(
                            "inputs = \"{}\" packages = [{}]",
                            rule.glob,
                            packages.join(", ")
                        ),
                        format!(
                            "limits a change to {} to selecting only {}",
                            rule.glob,
                            packages.join(", ").replace('"', "")
                        ),
                    )
                }));
                found.extend(globs("non_inputs", &config.non_inputs, &|glob| {
                    format!("stops a change to {glob} from selecting any package")
                }));
                found
            },
        );
    }
    let Ok(parsed) = table(bytes) else {
        return Vec::new();
    };
    if is_cargo_config(file) {
        cargo_config(&parsed, &make)
    } else if named(file, MANIFEST) {
        manifest(&parsed, &make)
    } else if MISE_CONFIGS.contains(file) {
        mise(&parsed, &make)
    } else if is_nextest_config(file) {
        nextest(&parsed, &make)
    } else {
        Vec::new()
    }
}

type Make<'a> = dyn Fn(String, String) -> Weakening + 'a;

fn manifest(parsed: &toml::Table, make: &Make<'_>) -> Vec<Weakening> {
    let package = parsed.get("package");
    let name = package
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str);
    let lib = parsed.get("lib");
    let off = |table: Option<&Value>, key: &str| {
        table
            .and_then(|table| table.get(key))
            .and_then(Value::as_bool)
            == Some(false)
    };
    let mut found = Vec::new();
    if let (true, Some(name)) = (off(lib, "doctest"), name) {
        found.push(make(
            "[lib] doctest = false".to_owned(),
            format!("turns off doctests for {name}"),
        ));
    }
    let owner = name.unwrap_or("this manifest");
    if off(lib, "test") {
        found.push(make(
            "[lib] test = false".to_owned(),
            format!("leaves the library unit tests of {owner} out of the test run"),
        ));
    }
    if off(package, "autotests") {
        found.push(make(
            "autotests = false".to_owned(),
            format!("stops {owner} from finding the files in tests/ as tests"),
        ));
    }
    for kind in ["test", "bench", "bin", "example"] {
        let targets = parsed
            .get(kind)
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for target in targets.filter(|target| off(Some(target), "test")) {
            let target_name = target
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unnamed");
            found.push(make(
                format!("[[{kind}]] name = \"{target_name}\" test = false"),
                format!("leaves the {kind} target {target_name} of {owner} out of the test run"),
            ));
        }
    }
    found
}

fn cargo_config(parsed: &toml::Table, make: &Make<'_>) -> Vec<Weakening> {
    let mut found = Vec::new();
    let build = parsed.get("build").and_then(Value::as_table);
    for key in COMPILER_KEYS {
        if let Some(value) = build.and_then(|build| build.get(key)) {
            if key.ends_with("wrapper") && value.as_str() == Some("") {
                continue;
            }
            let effect = if key.ends_with("wrapper") {
                format!("runs the compiler through {value}")
            } else {
                format!("runs {value} instead of {key}")
            };
            found.push(make(format!("build.{key} = {value}"), effect));
        }
    }
    let targets = parsed
        .get("target")
        .and_then(Value::as_table)
        .into_iter()
        .flatten();
    for (triple, section) in targets {
        if let Some(runner) = section.get("runner") {
            found.push(make(
                format!("target.{triple}.runner = {runner}"),
                format!("runs test binaries through {runner}"),
            ));
        }
    }
    let sections = build.into_iter().chain(
        parsed
            .get("target")
            .and_then(Value::as_table)
            .into_iter()
            .flat_map(|targets| targets.values().filter_map(Value::as_table)),
    );
    let flags: Vec<String> = sections
        .filter_map(|section| section.get("rustdocflags"))
        .flat_map(flag_tokens)
        .collect();
    if flags.iter().any(|flag| is_flag(flag, "--test-args")) {
        found.push(make(
            "rustdocflags with --test-args".to_owned(),
            "passes --test-args to rustdoc, which can leave out every doctest".to_owned(),
        ));
    }
    if flags.iter().any(|flag| is_flag(flag, "--no-run")) {
        found.push(make(
            "rustdocflags with --no-run".to_owned(),
            "passes --no-run to rustdoc, which compiles every doctest without running it"
                .to_owned(),
        ));
    }
    found
}

fn flag_tokens(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Value::String(text) => text.split_whitespace().map(str::to_owned).collect(),
        _ => Vec::new(),
    }
}

fn is_flag(token: &str, flag: &str) -> bool {
    token == flag
        || token
            .strip_prefix(flag)
            .is_some_and(|rest| rest.starts_with('='))
}

fn mise(parsed: &toml::Table, make: &Make<'_>) -> Vec<Weakening> {
    parsed
        .get("wrappers")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(name, _)| matches!(name.as_str(), "rustc" | "rustdoc") || cargo_tool(name))
        .map(|(name, wrapper)| {
            let command = wrapper
                .get("command")
                .map_or_else(|| "an unnamed command".to_owned(), ToString::to_string);
            make(
                format!("[wrappers.{name}] command = {command}"),
                format!("runs {name} through {command}"),
            )
        })
        .collect()
}

fn cargo_tool(name: &str) -> bool {
    name == "cargo" || name.starts_with("cargo-")
}

fn nextest(parsed: &toml::Table, make: &Make<'_>) -> Vec<Weakening> {
    let commands = parsed
        .get("scripts")
        .and_then(|scripts| scripts.get("wrapper"))
        .and_then(Value::as_table);
    let profiles = parsed
        .get("profile")
        .and_then(Value::as_table)
        .into_iter()
        .flatten();
    let filters: Vec<Weakening> = parsed
        .get("profile")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .filter(|(profile, _)| matches!(profile.as_str(), "default" | "ci"))
        .filter_map(|(profile, section)| {
            section
                .get("default-filter")
                .map(|filter| (profile, filter))
        })
        .map(|(profile, filter)| {
            make(
                format!("profile.{profile}.default-filter = {filter}"),
                format!(
                    "limits a plain nextest run to the tests matching {filter}; the nextest step Velnor generates ignores it"
                ),
            )
        })
        .collect();
    profiles
        .flat_map(|(profile, section)| {
            section
                .get("scripts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |entry| {
                    entry
                        .get("run-wrapper")
                        .and_then(Value::as_str)
                        .map(|name| (profile, name))
                })
        })
        .map(|(profile, name)| {
            let command = commands
                .and_then(|commands| commands.get(name))
                .and_then(|wrapper| wrapper.get("command"))
                .map_or_else(|| "an unnamed command".to_owned(), ToString::to_string);
            make(
                format!("profile.{profile}.scripts run-wrapper = \"{name}\""),
                format!("runs test binaries through the wrapper {name}, command {command}"),
            )
        })
        .chain(filters)
        .collect()
}

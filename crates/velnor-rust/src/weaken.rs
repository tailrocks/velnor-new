// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use toml::Value;
use velnor_model::{
    ChangeSet, DocsRun, MbxMode, PathGlob, RepoFacts, RepoPath, RustRoot, Weakening,
};

use crate::cargo_flags::flag_tokens;
use crate::config::{CONFIG_PATH, parse};
use crate::mise::MISE_CONFIGS;
use crate::roots::is_cargo_config;
use crate::toml_file::table;
use crate::weaken_nextest::nextest;
use crate::weaken_scope::reaches_a_check;
use crate::weaken_targets::targets;

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

pub(crate) fn is_manifest(path: &RepoPath) -> bool {
    named(path, MANIFEST)
}

pub(crate) fn weakenings(
    files: &BTreeMap<RepoPath, Vec<u8>>,
    roots: &[RustRoot],
) -> Vec<Weakening> {
    let mut found: Vec<Weakening> = files
        .iter()
        .filter(|(file, _)| reaches_a_check(file, roots))
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
        &facts.rust_roots,
    );
    facts
        .weakenings
        .iter()
        .filter(|weakening| {
            changes.paths.contains(&weakening.file)
                && !before
                    .iter()
                    .any(|old| old.file == weakening.file && old.setting == weakening.setting)
        })
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
                        .collect::<BTreeSet<_>>()
                        .into_iter()
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
                if config.docs_run == DocsRun::PostMerge {
                    found.push(make(
                        "docs = \"post_merge\"".to_owned(),
                        "skips Documentation on pull requests and in the merge queue, so a rustdoc failure is found only after the merge".to_owned(),
                    ));
                }
                if config.check_alone_run == DocsRun::PostMerge {
                    found.push(make(
                        "check_alone = \"post_merge\"".to_owned(),
                        "skips the per-crate alone checks on pull requests and in the merge queue, so a missing feature declaration is found only after the merge".to_owned(),
                    ));
                }
                if config.mbx == MbxMode::Off {
                    found.push(make(
                        "cache.mbx = false".to_owned(),
                        "runs cargo outside mbx, so no compiler results are shared and the mise cargo wrapper is not checked".to_owned(),
                    ));
                }
                found
            },
        );
    }
    let Ok(parsed) = table(bytes) else {
        return Vec::new();
    };
    if is_cargo_config(file) {
        cargo_config(&parsed, &make)
    } else if is_manifest(file) {
        manifest(&parsed, &make)
    } else if MISE_CONFIGS.contains(file) {
        mise(&parsed, &make)
    } else if is_nextest_config(file) {
        nextest(&parsed, &make)
    } else {
        Vec::new()
    }
}

pub(crate) type Make<'a> = dyn Fn(String, String) -> Weakening + 'a;

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
    if off(package, "build") {
        found.push(make(
            "build = false".to_owned(),
            format!("stops {owner} from compiling and running its build script"),
        ));
    }
    if off(package, "autotests") {
        found.push(make(
            "autotests = false".to_owned(),
            format!("stops {owner} from finding the files in tests/ as tests"),
        ));
    }
    for kind in ["test", "bin"] {
        let targets = parsed
            .get(kind)
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for (index, target) in targets.enumerate() {
            if !off(Some(target), "test") {
                continue;
            }
            let target_name = target
                .get("name")
                .or_else(|| target.get("path"))
                .and_then(Value::as_str)
                .map_or_else(|| format!("unnamed #{index}"), ToOwned::to_owned);
            found.push(make(
                format!("[[{kind}]] name = \"{target_name}\" test = false"),
                format!("leaves the {kind} target {target_name} of {owner} out of the test run"),
            ));
        }
    }
    found.extend(targets(parsed, owner, make));
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
            let selected = if selects(build, triple) {
                " (selected by build.target)"
            } else {
                ""
            };
            found.push(make(
                format!("target.{triple}.runner = {runner}{selected}"),
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
    let flags: Vec<(String, Vec<String>)> = sections
        .filter_map(|section| section.get("rustdocflags"))
        .map(|value| (value.to_string(), flag_tokens(value)))
        .collect();
    if let Some((flags, _)) = flags
        .iter()
        .find(|(_, tokens)| tokens.iter().any(|token| is_flag(token, "--test-args")))
    {
        found.push(make(
            format!("rustdocflags {flags} with --test-args"),
            "passes --test-args to rustdoc, which can leave out every doctest".to_owned(),
        ));
    }
    if let Some((flags, _)) = flags
        .iter()
        .find(|(_, tokens)| tokens.iter().any(|token| is_flag(token, "--no-run")))
    {
        found.push(make(
            format!("rustdocflags {flags} with --no-run"),
            "passes --no-run to rustdoc, which compiles every doctest without running it"
                .to_owned(),
        ));
    }
    if let Some((flags, _)) = flags
        .iter()
        .find(|(_, tokens)| tokens.iter().any(|token| is_flag(token, "--test-runtool")))
    {
        found.push(make(
            format!("rustdocflags {flags} with --test-runtool"),
            "passes --test-runtool to rustdoc, which runs every doctest through another program"
                .to_owned(),
        ));
    }
    found
}

fn selects(build: Option<&toml::Table>, triple: &str) -> bool {
    match build.and_then(|build| build.get("target")) {
        Some(Value::String(one)) => one == triple,
        Some(Value::Array(many)) => many.iter().any(|item| item.as_str() == Some(triple)),
        _ => false,
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
        .filter(|(name, wrapper)| !standard_mbx(name, wrapper))
        .map(|(name, wrapper)| {
            let command = wrapper
                .get("command")
                .map_or_else(|| "an unnamed command".to_owned(), ToString::to_string);
            let extra: String = ["args", "env"]
                .iter()
                .filter_map(|key| wrapper.get(key).map(|value| format!(" {key} = {value}")))
                .collect();
            make(
                format!("[wrappers.{name}] command = {command}{extra}"),
                format!("runs {name} through {command}"),
            )
        })
        .collect()
}

fn cargo_tool(name: &str) -> bool {
    name == "cargo" || name.starts_with("cargo-")
}

/// The owner standard MBX layout (`mise.toml`: `[wrappers.cargo]` with exactly
/// `command = "mbx"` and `env = { MBX_CARGO_SHIM_MODE = "1" }`). Any other key,
/// value or command is a different wrapper and stays named.
fn standard_mbx(name: &str, wrapper: &Value) -> bool {
    name == "cargo" && crate::mbx_layout::standard_wrapper(wrapper)
}

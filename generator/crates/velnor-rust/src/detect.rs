// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::toml_file::table;
use velnor_model::{
    PackageName, PolicyTools, RELEASE_CONFIG_PATH, RELEASE_PLZ_FILE, RepoFacts, RepoPath, RepoView,
    RootSlug, RustCiConfig, RustRoot, Toolchain,
};

use crate::config::{CONFIG_PATH, excluded, parse};
use crate::error::{DetectError, Reason};
use crate::lock::{LOCK_PATH, parse_lock};
use crate::mise::{MISE_CONFIGS, MISE_LOCK, tool_pins};
use crate::os_pins::pinned_for_runner;
use crate::policy::DENY_CONFIG;
use crate::reads::{INPUTS_LOCK, parse_inputs_lock};
use crate::roots::{CARGO_LOCK, effective_configs, is_cargo_config, rust_roots};
use crate::timings::{TIMINGS_PATH, usable_timings};
use crate::toolchain::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML, toolchain};
use crate::unowned::unowned;
use crate::weaken::{is_nextest_config, weakenings};

pub(crate) const HELPER_PACKAGE: &str = "velnor-ci";
const CARGO_MANIFEST: &str = "Cargo.toml";

#[must_use]
pub fn wanted_files(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    [
        CONFIG_PATH,
        RELEASE_CONFIG_PATH,
        LOCK_PATH,
        INPUTS_LOCK,
        TIMINGS_PATH,
        MISE_LOCK,
        TOOLCHAIN_PLAIN,
        TOOLCHAIN_TOML,
    ]
    .into_iter()
    .chain(MISE_CONFIGS.iter().cloned())
    .filter(|path| tracked.contains(path))
    .chain(
        tracked
            .iter()
            .filter(|path| is_cargo_config(path) || is_nextest_config(path))
            .cloned(),
    )
    .chain(
        tracked
            .iter()
            .filter(|path| is_nested_toolchain(path))
            .cloned(),
    )
    .chain(
        tracked
            .iter()
            .filter(|path| is_release_plz_config(path))
            .cloned(),
    )
    .chain(
        tracked
            .iter()
            .filter(|path| is_workflow_file(path))
            .cloned(),
    )
    .collect()
}

fn is_nested_toolchain(path: &RepoPath) -> bool {
    path.as_str().rsplit_once('/').is_some_and(|(_, name)| {
        name == TOOLCHAIN_PLAIN.as_str() || name == TOOLCHAIN_TOML.as_str()
    })
}

fn is_workflow_file(path: &RepoPath) -> bool {
    path.as_str()
        .strip_prefix(".github/workflows/")
        .is_some_and(|name| {
            !name.contains('/')
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
}

fn is_release_plz_config(path: &RepoPath) -> bool {
    path.as_str().rsplit('/').next() == Some(RELEASE_PLZ_FILE)
}

pub fn metadata_roots(view: &RepoView) -> Result<Vec<RepoPath>, DetectError> {
    let config = config(view)?;
    let mut manifests: Vec<RepoPath> = tracked_manifests(&view.tracked_paths)
        .filter(|manifest| !excluded(&config, manifest))
        .cloned()
        .collect();
    manifests.sort_by_key(|manifest| (manifest.as_str().matches('/').count(), manifest.clone()));
    Ok(manifests)
}

pub fn detect(view: &RepoView) -> Result<RepoFacts, DetectError> {
    let config = config(view)?;
    let (rust_roots, targets) = rust_roots(view, &config)?;
    let unowned = unowned(view, &rust_roots, &targets);
    let mut slugs = BTreeMap::new();
    for root in &rust_roots {
        if let Some(first) = slugs.insert(RootSlug::of(&root.manifest), &root.manifest) {
            return Err(DetectError::SameSlug {
                first: first.clone(),
                second: root.manifest.clone(),
            });
        }
    }
    let compile_inputs = view
        .files
        .get(&INPUTS_LOCK)
        .map_or_else(|| Ok(BTreeMap::new()), |bytes| parse_inputs_lock(bytes))?;
    let named = config.inputs.iter().flat_map(|input| &input.packages);
    if let Some(package) = unknown_package(&rust_roots, named) {
        return Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::UnknownInputPackage(package),
        ));
    }
    if let Some(package) = unknown_package(&rust_roots, compile_inputs.values().flatten()) {
        return Err(DetectError::invalid(
            &INPUTS_LOCK,
            Reason::UnknownLockedPackage(package),
        ));
    }
    check_nextest_harnesses(&config, &rust_roots)?;
    check_helper_box(&config, &rust_roots)?;
    let toolchain = toolchain(&view.files)?;
    let tools = tool_pins(&view.files)?;
    check_setup_files(view, &rust_roots)?;
    check_build_targets(view, toolchain.as_ref())?;
    let helper = view
        .files
        .get(&LOCK_PATH)
        .map(|bytes| {
            parse_lock(bytes)
                .map_err(|error| DetectError::invalid(&LOCK_PATH, Reason::HelperLock(error)))
        })
        .transpose()?;
    if config.helper.is_some()
        && !helper
            .as_ref()
            .is_some_and(|release| release.version().at_least((0, 1, 81)))
    {
        return Err(DetectError::invalid(
            &LOCK_PATH,
            Reason::HelperSourceRelease,
        ));
    }
    let (timings, timings_notes) = usable_timings(view.files.get(&TIMINGS_PATH), &rust_roots);
    let policy = policy_tools(view, config.runner.as_str())?;
    let weakenings = weakenings(&view.files, &rust_roots);
    Ok(RepoFacts {
        policy,
        repository: view.repository.clone(),
        timings,
        timings_notes,
        rust_roots,
        toolchain,
        tools,
        config,
        helper,
        compile_inputs,
        unowned,
        weakenings,
    })
}

const DENY_TOOL: &str = "cargo-deny";

fn policy_tools(view: &RepoView, runner: &str) -> Result<PolicyTools, DetectError> {
    let pinned = |tool: &str| pinned_for_runner(&view.files, |name| is_tool(name, tool), runner);
    let deny = view.tracked_paths.contains(&DENY_CONFIG);
    if deny && !pinned(DENY_TOOL) {
        return Err(DetectError::invalid(
            &DENY_CONFIG,
            Reason::UnpinnedTool(DENY_TOOL),
        ));
    }
    Ok(PolicyTools {
        deny,
        machete: pinned("cargo-machete"),
    })
}

fn is_tool(name: &str, tool: &str) -> bool {
    name.rsplit(['/', ':']).next() == Some(tool)
}

fn check_helper_box(config: &RustCiConfig, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    let Some(path) = &config.helper else {
        return Ok(());
    };
    let root = rust_roots.iter().find(|root| {
        root.manifest.parent().as_ref() == Some(path)
            && root
                .members
                .iter()
                .any(|member| member.name.as_str() == HELPER_PACKAGE)
    });
    match root {
        None => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::HelperBox(path.clone()),
        )),
        Some(_) => Ok(()),
    }
}

fn check_build_targets(view: &RepoView, toolchain: Option<&Toolchain>) -> Result<(), DetectError> {
    let declared: Vec<&String> = toolchain.into_iter().flat_map(|t| &t.targets).collect();
    for (file, bytes) in effective_configs(&view.files) {
        let parsed = table(bytes).map_err(|reason| DetectError::invalid(file, reason))?;
        let target = parsed.get("build").and_then(|build| build.get("target"));
        let named: Vec<String> = match target {
            Some(toml::Value::String(one)) => vec![one.clone()],
            Some(toml::Value::Array(many)) => many
                .iter()
                .map(|item| item.as_str().unwrap_or_default().to_owned())
                .collect(),
            _ => Vec::new(),
        };
        if let Some(missing) = named.into_iter().find(|name| !declared.contains(&name)) {
            return Err(DetectError::invalid(
                file,
                Reason::UndeclaredTarget(missing),
            ));
        }
    }
    Ok(())
}

fn check_setup_files(view: &RepoView, rust_roots: &[RustRoot]) -> Result<(), DetectError> {
    if let Some(file) = view
        .tracked_paths
        .iter()
        .find(|path| other_mise_config(path))
    {
        return Err(DetectError::invalid(file, Reason::OtherMiseConfig));
    }
    let folders: Vec<RepoPath> = rust_roots
        .iter()
        .filter_map(|root| root.manifest.parent())
        .flat_map(|folder| std::iter::successors(Some(folder), RepoPath::parent))
        .collect();
    let nested = view
        .tracked_paths
        .iter()
        .find(|path| {
            folders.iter().any(|folder| {
                path.as_str()
                    .strip_prefix(&format!("{folder}/"))
                    .and_then(|rest| RepoPath::new(rest).ok())
                    .is_some_and(|rest| {
                        other_mise_config(&rest)
                            || MISE_CONFIGS.contains(&rest)
                            || rest == TOOLCHAIN_PLAIN
                            || rest == TOOLCHAIN_TOML
                    })
            })
        })
        .cloned();
    if let Some(file) = nested {
        return Err(DetectError::NestedToolchain { file });
    }
    Ok(())
}

const MISE_FOLDERS: [&str; 3] = [".config/", ".mise/", "mise/"];

fn other_mise_config(path: &RepoPath) -> bool {
    let text = path.as_str();
    let toml = std::path::Path::new(text)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"));
    let named = |name: &str| {
        ["mise", ".mise", "rtx", ".rtx"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
    };
    let at_root = !text.contains('/') && named(text) && toml;
    let in_folder = MISE_FOLDERS.iter().any(|folder| {
        text.strip_prefix(folder)
            .is_some_and(|rest| toml && (*folder != ".config/" || named(rest)))
    });
    text == ".tool-versions"
        || (at_root && !MISE_CONFIGS.iter().any(|config| config == path))
        || in_folder
}

fn unknown_package<'p>(
    roots: &[RustRoot],
    mut packages: impl Iterator<Item = &'p PackageName>,
) -> Option<String> {
    packages
        .find(|package| {
            !roots
                .iter()
                .flat_map(|root| &root.members)
                .any(|member| member.name == **package)
        })
        .map(ToString::to_string)
}

fn config(view: &RepoView) -> Result<RustCiConfig, DetectError> {
    let config = view
        .files
        .get(&CONFIG_PATH)
        .map_or_else(|| Ok(RustCiConfig::default()), |bytes| parse(bytes))?;
    let manifests: Vec<&RepoPath> = tracked_manifests(&view.tracked_paths).collect();
    match config
        .exclude
        .iter()
        .find(|entry| !manifests.iter().any(|manifest| entry.matches(manifest)))
    {
        Some(entry) => Err(DetectError::invalid(
            &CONFIG_PATH,
            Reason::UnmatchedExclude(entry.clone()),
        )),
        None => Ok(config),
    }
}

#[must_use]
pub fn cargo_files(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    tracked
        .iter()
        .filter(|path| is_named(path, CARGO_MANIFEST) || is_named(path, CARGO_LOCK.as_str()))
        .cloned()
        .collect()
}

#[must_use]
pub fn manifests(tracked: &BTreeSet<RepoPath>) -> Vec<RepoPath> {
    tracked_manifests(tracked).cloned().collect()
}

fn tracked_manifests(tracked: &BTreeSet<RepoPath>) -> impl Iterator<Item = &RepoPath> {
    tracked.iter().filter(|path| is_named(path, CARGO_MANIFEST))
}

fn is_named(path: &RepoPath, name: &str) -> bool {
    path.as_str() == name
        || path
            .as_str()
            .strip_suffix(name)
            .is_some_and(|rest| rest.ends_with('/'))
}

fn check_nextest_harnesses(
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
    for (package, target) in &config.nextest_harnesses {
        let declared: usize = rust_roots
            .iter()
            .flat_map(|root| &root.members)
            .filter(|member| member.name == *package)
            .map(|member| {
                member
                    .custom_harnesses
                    .iter()
                    .filter(|harness| harness.name == *target)
                    .count()
            })
            .sum();
        let entry = format!("{package}:{target}");
        let reason = match declared {
            0 => Reason::UnknownNextestHarness(entry),
            1 => continue,
            _ => Reason::AmbiguousNextestHarness(entry),
        };
        return Err(DetectError::invalid(&CONFIG_PATH, reason));
    }
    Ok(())
}

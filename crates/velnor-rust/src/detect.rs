// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{
    PackageName, RELEASE_CONFIG_PATH, RELEASE_PLZ_FILE, RepoFacts, RepoPath, RepoView,
    RepositoryIdentity, RootSlug, RustCiConfig, RustRoot, Toolchain, WORKFLOWS_CONFIG_PATH,
};

use crate::build_target::{build_targets, check_host_tuple_version};
use crate::config::{CONFIG_PATH, excluded, parse};
use crate::embedded::is_rust_source;
use crate::error::{DetectError, Reason};
use crate::lock::{LOCK_PATH, parse_lock};
use crate::mise::{MISE_CONFIGS, MISE_LOCK};
use crate::policy::{deny_configs, policy_tools};
use crate::private_sources::{private_suggestion, refuse_locked_credentials};
use crate::reads::INPUTS_LOCK;
use crate::roots::{CARGO_LOCK, is_cargo_config, is_cargo_lock, rust_roots};
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
        WORKFLOWS_CONFIG_PATH,
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
            .filter(|path| {
                is_cargo_config(path)
                    || is_nextest_config(path)
                    || is_cargo_lock(path)
                    || is_rust_source(path)
            })
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
    let compile_inputs = crate::embedded::locked_and_embedded(view, &rust_roots)?;
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
    crate::config_test_groups::check_tests(&config, &rust_roots)?;
    crate::groups::check(view, &config, &rust_roots)?;
    crate::groups::check_shape(&config)?;
    crate::config_test_lane::check(&config, &rust_roots)?;
    let toolchain = toolchain(&view.files)?;
    let tools = crate::config_lint::pinned_tools(view, &config)?;
    check_setup_files(view, &rust_roots)?;
    check_build_targets(view, &rust_roots, toolchain.as_ref(), &tools)?;
    let helper = view
        .files
        .get(&LOCK_PATH)
        .map(|bytes| {
            parse_lock(bytes)
                .map_err(|error| DetectError::invalid(&LOCK_PATH, Reason::HelperLock(error)))
        })
        .transpose()?;
    crate::groups::check_helper(&config, helper.as_ref())?;
    refuse_locked_credentials(view, &config, &rust_roots)?;
    let (timings, timings_notes) = usable_timings(view.files.get(&TIMINGS_PATH), &rust_roots);
    let deny_configs = deny_configs(view, &rust_roots);
    let policy = policy_tools(view, &deny_configs, config.runner.as_str())?;
    let weakenings = weakenings(&view.files, &rust_roots);
    let private_sources = crate::config_cache::private_sources(view, &rust_roots, &config)?;
    let repository = repository_identity(view, &config)?;
    Ok(RepoFacts {
        policy,
        deny_configs,
        repository,
        timings,
        timings_notes,
        private_sources,
        private_suggestion: private_suggestion(view, &config, &rust_roots),
        weakenings,
        rust_roots,
        toolchain,
        tools,
        config,
        helper,
        compile_inputs,
        unowned,
    })
}

fn repository_identity(view: &RepoView, config: &RustCiConfig) -> Result<String, DetectError> {
    let hashed = config.full_run.is_hashed();
    match &view.repository {
        RepositoryIdentity::Named(name) if hashed => Ok(name.clone()),
        RepositoryIdentity::Unhosted if hashed => Err(DetectError::UnhostedOrigin),
        _ => Ok(String::new()),
    }
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

fn check_build_targets(
    view: &RepoView,
    rust_roots: &[RustRoot],
    toolchain: Option<&Toolchain>,
    tools: &velnor_model::ToolPins,
) -> Result<(), DetectError> {
    let declared: Vec<&String> = toolchain.into_iter().flat_map(|t| &t.targets).collect();
    let rust_version = crate::mise::rust_tool(tools)
        .and_then(|name| tools.tools.get(name))
        .and_then(|pin| pin.requested.first())
        .map(String::as_str)
        .or(toolchain.and_then(|pinned| pinned.channel.as_deref().or(pinned.path.as_deref())))
        .unwrap_or("an unspecified Rust toolchain");
    for root in rust_roots {
        check_host_tuple_version(view, &root.manifest, rust_version)?;
        let missing = build_targets(view, &root.manifest)?
            .into_iter()
            .find(|(_, name)| !declared.contains(&name));
        if let Some((file, name)) = missing {
            return Err(DetectError::invalid(&file, Reason::UndeclaredTarget(name)));
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

const MISE_FOLDERS: [&str; 5] = [".config/", ".mise/", "mise/", "rtx/", ".rtx/"];

fn other_mise_config(path: &RepoPath) -> bool {
    let text = path.as_str();
    let toml = std::path::Path::new(text)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"));
    let named = |name: &str| {
        ["mise", ".mise", "rtx", ".rtx"].iter().any(|prefix| {
            name.strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with(['.', '/']))
        })
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

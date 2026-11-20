// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView, RustCiConfig, RustRoot, ToolPins, Toolchain};

use crate::build_target::{build_targets, check_host_tuple_version};
use crate::config::CONFIG_PATH;
use crate::detect::HELPER_PACKAGE;
use crate::error::{DetectError, Reason};
use crate::mise::MISE_CONFIGS;
use crate::toolchain::{TOOLCHAIN_PLAIN, TOOLCHAIN_TOML};

pub(crate) fn check_helper_box(
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
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

pub(crate) fn check_build_targets(
    view: &RepoView,
    rust_roots: &[RustRoot],
    toolchain: Option<&Toolchain>,
    tools: &ToolPins,
) -> Result<(), DetectError> {
    let declared: Vec<&String> = toolchain.into_iter().flat_map(|t| &t.targets).collect();
    let rust_version = crate::mise::rust_tool(tools)
        .and_then(|name| tools.tools.get(name))
        .and_then(|pin| pin.requested.first())
        .map(|request| request.version.as_str())
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

pub(crate) fn check_setup_files(
    view: &RepoView,
    rust_roots: &[RustRoot],
) -> Result<(), DetectError> {
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

const MISE_FOLDERS: &[&str] = &[".config/", ".mise/", "mise/", "rtx/", ".rtx/"];

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

pub(crate) fn check_nextest_harnesses(
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

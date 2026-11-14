// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_model::{AllTrigger, PackageName, RepoPath, RustCiConfig, WORKFLOW_PATH};

use crate::config::{CONFIG_PATH, rule_matches};
use crate::reads::INPUTS_LOCK;
use crate::roots::{CARGO_LOCK, is_cargo_config};

pub(crate) enum Class<'c> {
    Nothing,
    Packages(Vec<&'c PackageName>),
    Global(AllTrigger),
    Unknown,
}

const TOOLCHAIN_FILES: [&str; 2] = ["rust-toolchain", "rust-toolchain.toml"];
const TOOL_PIN_FILES: [&str; 3] = ["mise.toml", ".mise.toml", "mise.lock"];
const IGNORED_FOLDERS: [&str; 3] = ["docs/", "LICENSES/", ".github/ISSUE_TEMPLATE/"];
const IGNORED_ROOT_NAMES: [&str; 2] = ["NOTICE", "CODEOWNERS"];
const IGNORED_ROOT_PREFIX: &str = "LICENSE";
const PULL_REQUEST_TEMPLATE: &str = ".github/PULL_REQUEST_TEMPLATE";

pub(crate) fn classify<'c>(
    config: &'c RustCiConfig,
    path: &RepoPath,
    is_root_manifest: bool,
    sheltered: bool,
) -> Class<'c> {
    if config
        .non_inputs
        .iter()
        .any(|glob| rule_matches(glob, path))
    {
        return Class::Nothing;
    }
    let packages: Vec<&PackageName> = config
        .inputs
        .iter()
        .filter(|input| rule_matches(&input.glob, path))
        .flat_map(|input| &input.packages)
        .collect();
    if !packages.is_empty() {
        return Class::Packages(packages);
    }
    if let Some(trigger) = global(path, is_root_manifest) {
        return Class::Global(trigger);
    }
    if config.default_ignores && !sheltered && ignored(path) {
        return Class::Nothing;
    }
    Class::Unknown
}

fn global(path: &RepoPath, is_root_manifest: bool) -> Option<AllTrigger> {
    let name = path.as_str().rsplit('/').next().unwrap_or_default();
    if TOOLCHAIN_FILES.contains(&name) {
        Some(AllTrigger::ToolchainFile(path.clone()))
    } else if is_cargo_config(path) {
        Some(AllTrigger::CargoConfig(path.clone()))
    } else if is_root_manifest {
        Some(AllTrigger::WorkspaceManifest)
    } else if *path == WORKFLOW_PATH {
        Some(AllTrigger::GeneratedWorkflow)
    } else if TOOL_PIN_FILES.contains(&name)
        || name == CARGO_LOCK.as_str()
        || *path == CONFIG_PATH
        || *path == INPUTS_LOCK
    {
        Some(AllTrigger::Global(path.clone()))
    } else {
        None
    }
}

fn ignored(path: &RepoPath) -> bool {
    let text = path.as_str();
    let at_root = !text.contains('/');
    IGNORED_FOLDERS
        .iter()
        .any(|folder| text.starts_with(folder))
        || text.starts_with(PULL_REQUEST_TEMPLATE)
        || (at_root
            && (IGNORED_ROOT_NAMES.contains(&text)
                || text.starts_with(IGNORED_ROOT_PREFIX)
                || Path::new(text)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))))
}

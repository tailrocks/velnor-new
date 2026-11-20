// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_model::{
    AllTrigger, ChangeSet, Owner, PackageName, RepoPath, RustCiConfig, WORKFLOW_PATH, owner_of,
};

use crate::config::{CONFIG_PATH, rule_matches};
use crate::lock::LOCK_PATH;
use crate::metadata_file::METADATA_PATH;
use crate::reads::INPUTS_LOCK;
use crate::roots::{CARGO_LOCK, is_cargo_config};
use crate::timings::TIMINGS_PATH;

pub(crate) enum Class<'c> {
    Nothing,
    Packages(Vec<&'c PackageName>),
    Global(AllTrigger),
    Unknown,
}

const TOOLCHAIN_FILES: &[&str] = &["rust-toolchain", "rust-toolchain.toml"];
const TOOL_PIN_FILES: &[&str] = &["mise.toml", ".mise.toml", "mise.lock"];
const IGNORED_FOLDERS: &[&str] = &["docs/", "LICENSES/", ".github/ISSUE_TEMPLATE/"];
const IGNORED_ROOT_NAMES: &[&str] = &["NOTICE", "CODEOWNERS"];
const IGNORED_ROOT_PREFIX: &str = "LICENSE";
const PULL_REQUEST_TEMPLATE: &str = ".github/PULL_REQUEST_TEMPLATE";

pub(crate) fn classify<'c>(
    config: &'c RustCiConfig,
    path: &RepoPath,
    is_root_manifest: bool,
    sheltered: bool,
    generated: bool,
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
    if config
        .check_configurations
        .iter()
        .any(|check| check.file().as_ref() == Some(path))
    {
        return Class::Unknown;
    }
    // velnor-gen owns this workflow and it runs no Rust code; rust-ci.yml is matched above.
    if generated {
        return Class::Nothing;
    }
    if config.default_ignores && !sheltered && ignored(path) {
        return Class::Nothing;
    }
    Class::Unknown
}

/// A workflow file: its content can carry the velnor-gen header.
#[must_use]
pub fn is_workflow_file(path: &RepoPath) -> bool {
    let text = path.as_str();
    text.starts_with(".github/workflows/")
        && Path::new(text).extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yml") || extension.eq_ignore_ascii_case("yaml")
        })
}

/// True when `path` is a workflow that velnor-gen owns, judged by its sealed
/// header in the head version or, for a deleted file, the base version.
pub(crate) fn generated_workflow(changes: &ChangeSet, path: &RepoPath) -> bool {
    is_workflow_file(path)
        && changes
            .head_files
            .get(path)
            .or_else(|| changes.base_files.get(path))
            .is_some_and(|text| owner_of(text.as_bytes()) == Owner::VelnorGen)
}

/// A nextest config (`.config/nextest*.toml`) or a script next to it (`.config/*.sh`, such as
/// one a `[scripts.setup.*]` command runs): it changes how every test runs.
fn is_test_runner_config(path: &RepoPath) -> bool {
    let Some((folder, name)) = path.as_str().rsplit_once('/') else {
        return false;
    };
    let extension = |wanted: &str| {
        Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(wanted))
    };
    (folder == ".config" || folder.ends_with("/.config"))
        && ((name.starts_with("nextest") && extension("toml")) || extension("sh"))
}

fn global(path: &RepoPath, is_root_manifest: bool) -> Option<AllTrigger> {
    let name = path.as_str().rsplit('/').next().unwrap_or_default();
    if TOOLCHAIN_FILES.contains(&name) {
        Some(AllTrigger::ToolchainFile(path.clone()))
    } else if is_cargo_config(path) {
        Some(AllTrigger::CargoConfig(path.clone()))
    } else if is_test_runner_config(path) {
        Some(AllTrigger::Global(path.clone()))
    } else if is_root_manifest {
        Some(AllTrigger::WorkspaceManifest)
    } else if *path == WORKFLOW_PATH {
        Some(AllTrigger::GeneratedWorkflow)
    } else if TOOL_PIN_FILES.contains(&name)
        || name == CARGO_LOCK.as_str()
        || *path == CONFIG_PATH
        || *path == LOCK_PATH
        || *path == TIMINGS_PATH
        || *path == INPUTS_LOCK
        || *path == METADATA_PATH
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

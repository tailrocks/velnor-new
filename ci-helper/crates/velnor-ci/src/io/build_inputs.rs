// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_gen::io::read_files;
use velnor_model::RepoPath;
use velnor_rust::{repo_profile, rustc_release};

use crate::error::CiError;

const CONFIG_NAMES: [&str; 2] = ["config", "config.toml"];
const OS_RELEASE: &str = "/etc/os-release";

pub(super) fn repository_profile(root: &Path) -> Result<String, CiError> {
    let mut paths = Vec::new();
    cargo_configs(root, root, &mut paths)?;
    let files = read_files(root, &paths).map_err(|e| CiError::new(e.to_string()))?;
    repo_profile(&files).map_err(|e| CiError::new(e.to_string()))
}

fn cargo_configs(root: &Path, folder: &Path, found: &mut Vec<RepoPath>) -> Result<(), CiError> {
    let listing = std::fs::read_dir(folder)
        .map_err(|e| CiError::new(format!("cannot read {}: {e}", folder.display())))?;
    for entry in listing {
        let entry = entry.map_err(|e| CiError::new(e.to_string()))?;
        let kind = entry.file_type().map_err(|e| CiError::new(e.to_string()))?;
        let name = entry.file_name();
        let path = entry.path();
        if kind.is_dir() && name != ".git" {
            cargo_configs(root, &path, found)?;
        } else if kind.is_file()
            && CONFIG_NAMES.iter().any(|candidate| name == *candidate)
            && folder.file_name().is_some_and(|parent| parent == ".cargo")
        {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| CiError::new(e.to_string()))?;
            let text = relative
                .to_str()
                .ok_or_else(|| CiError::new("a cargo config path is not UTF-8".to_owned()))?;
            found.push(RepoPath::new(text).map_err(|e| CiError::new(e.to_string()))?);
        }
    }
    Ok(())
}

pub(super) fn release_of(verbose: &str) -> Result<(u32, u32), CiError> {
    rustc_release(verbose).ok_or_else(|| CiError::new("rustc -vV reports no release".to_owned()))
}

pub(super) fn image_identity() -> String {
    let image_os = std::env::var("ImageOS").unwrap_or_default();
    let os_release = std::fs::read_to_string(OS_RELEASE).unwrap_or_default();
    let field = |name: &str| {
        os_release
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .map_or_else(String::new, |value| value.trim_matches('"').to_owned())
    };
    format!("{image_os}|{}|{}", field("ID"), field("VERSION_ID"))
}

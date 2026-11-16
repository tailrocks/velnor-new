// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

use velnor_gen::io::read_files;
use velnor_model::RepoPath;
use velnor_rust::{
    METADATA_PATH, cargo_config_paths, check_runtime_configs, recorded_roots, repo_profile,
    rustc_release,
};

use crate::error::CiError;

const OS_RELEASE: &str = "/etc/os-release";

pub(super) fn repository_profile(root: &Path) -> Result<String, CiError> {
    let recorded = read_files(root, &[METADATA_PATH]).map_err(|e| CiError::new(e.to_string()))?;
    let roots = recorded_roots(recorded.get(&METADATA_PATH).map(Vec::as_slice))
        .map_err(|e| CiError::new(e.to_string()))?;
    let inside = root
        .canonicalize()
        .map_err(|e| CiError::new(format!("cannot resolve {}: {e}", root.display())))?;
    let mut files = BTreeMap::new();
    for path in cargo_config_paths(&roots) {
        if let Some(bytes) = read_config(root, &inside, &path)? {
            files.insert(path, bytes);
        }
    }
    let tracked = tracked_among(root, &files);
    check_runtime_configs(&roots, &files, &tracked).map_err(|e| CiError::new(e.to_string()))?;
    repo_profile(&files).map_err(|e| CiError::new(e.to_string()))
}

fn tracked_among(root: &Path, files: &BTreeMap<RepoPath, Vec<u8>>) -> BTreeSet<RepoPath> {
    let Some(listed) = listed_configs(root, files) else {
        return BTreeSet::new();
    };
    let wanted: BTreeSet<&str> = files.keys().map(RepoPath::as_str).collect();
    listed
        .split(|byte| *byte == 0)
        .filter_map(|name| std::str::from_utf8(name).ok())
        .filter(|name| wanted.contains(name))
        .filter_map(|name| RepoPath::new(name).ok())
        .collect()
}

fn listed_configs(root: &Path, files: &BTreeMap<RepoPath, Vec<u8>>) -> Option<Vec<u8>> {
    if files.is_empty() {
        return Some(Vec::new());
    }
    let output = Command::new("git")
        .arg("--literal-pathspecs")
        .args(["ls-files", "-z", "--"])
        .args(files.keys().map(RepoPath::as_str))
        .current_dir(root)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn read_config(root: &Path, inside: &Path, path: &RepoPath) -> Result<Option<Vec<u8>>, CiError> {
    let file = root.join(path.as_str());
    let resolved = match file.canonicalize() {
        Ok(resolved) => resolved,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CiError::new(format!("cannot read {path}: {error}"))),
    };
    if !resolved.starts_with(inside) {
        return Err(CiError::new(format!(
            "{path} resolves to {}, outside the checkout",
            resolved.display()
        )));
    }
    std::fs::read(&resolved)
        .map(Some)
        .map_err(|e| CiError::new(format!("cannot read {path}: {e}")))
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

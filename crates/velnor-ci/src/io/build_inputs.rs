// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_gen::io::read_files;
use velnor_model::RepoPath;
use velnor_rust::{
    METADATA_PATH, cargo_config_paths, check_runtime_configs, external_config_profile,
    recorded_roots, recorded_target_dirs, repo_profile, rustc_release,
};

use crate::error::CiError;
use crate::escape::escape_data;

const OS_RELEASE: &str = "/etc/os-release";
const OS_RELEASE_OVERRIDE: &str = "VELNOR_OS_RELEASE";

pub(super) fn repository_profile(root: &Path) -> Result<String, CiError> {
    let recorded = read_files(root, &[METADATA_PATH]).map_err(|e| CiError::new(e.to_string()))?;
    let roots = recorded_roots(recorded.get(&METADATA_PATH).map(Vec::as_slice))
        .map_err(|e| CiError::new(e.to_string()))?;
    let recorded_dirs = recorded_target_dirs(recorded.get(&METADATA_PATH).map(Vec::as_slice))
        .map_err(|e| CiError::new(e.to_string()))?;
    let mut files = BTreeMap::new();
    for path in cargo_config_paths(&roots) {
        if let Some(bytes) = read_config(root, &path)? {
            files.insert(path, bytes);
        }
    }
    let tracked = tracked_among(root, &files);
    let targets = check_runtime_configs(&roots, &files, &tracked, &recorded_dirs)
        .map_err(|e| CiError::new(e.to_string()))?;
    for target in targets {
        super::delta_paths::untracked(&root.join(target.as_str()), "target")?;
    }
    let inside = repo_profile(&files).map_err(|e| CiError::new(e.to_string()))?;
    let outside = external_config_profile(&external_configs(root)?).map_err(CiError::new)?;
    Ok(format!("{inside}{outside}"))
}

fn external_configs(root: &Path) -> Result<Vec<(String, Vec<u8>)>, CiError> {
    let rewritten = root
        .parent()
        .map(|parent| parent.join(".cargo").join("config.toml"));
    let mut folders: Vec<PathBuf> = root
        .ancestors()
        .skip(1)
        .map(|folder| folder.join(".cargo"))
        .collect();
    let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    let home = set("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| set("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    if let Some(home) = home
        && !folders.contains(&home)
    {
        folders.push(home);
    }
    let mut found = Vec::new();
    for folder in folders {
        if let Some((file, bytes)) = effective_config(&folder, rewritten.as_deref())? {
            found.push((lossless(&file), bytes));
        }
    }
    Ok(found)
}

/// The one file Cargo reads from `folder`: the legacy `config` shadows `config.toml`.
/// The config this invocation rewrites is exempt; a marked file anywhere else is hashed.
fn effective_config(
    folder: &Path,
    rewritten: Option<&Path>,
) -> Result<Option<(PathBuf, Vec<u8>)>, CiError> {
    for name in ["config", "config.toml"] {
        let file = folder.join(name);
        match std::fs::read(&file) {
            Ok(bytes)
                if rewritten == Some(file.as_path())
                    && bytes.starts_with(super::setup::OWNED_MARKER.as_bytes()) =>
            {
                return Ok(None);
            }
            Ok(bytes) => return Ok(Some((file, bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(CiError::new(format!(
                    "cannot read {}: {e}; the base cache key must cover every Cargo config Cargo reads",
                    file.display()
                )));
            }
        }
    }
    Ok(None)
}

/// A path as text without the lossy replacement of `Path::display`: two different non-UTF-8
/// paths must not hash alike. A NUL cannot occur in a path, so the encoded form cannot
/// collide with a valid one.
fn lossless(path: &Path) -> String {
    path.to_str().map_or_else(
        || {
            let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str());
            let mut text = String::from("\0bytes:");
            for byte in bytes {
                use std::fmt::Write as _;
                let _ = write!(text, "{byte:02x}");
            }
            text
        },
        str::to_owned,
    )
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

fn read_config(root: &Path, path: &RepoPath) -> Result<Option<Vec<u8>>, CiError> {
    let mut read = read_files(root, std::slice::from_ref(path))
        .map_err(|error| CiError::new(error.to_string()))?;
    Ok(read.remove(path))
}

pub(super) fn release_of(verbose: &str) -> Result<(u32, u32), CiError> {
    rustc_release(verbose).ok_or_else(|| CiError::new("rustc -vV reports no release".to_owned()))
}

/// The runner image identity, or, when `/etc/os-release` cannot be read, an identity no other
/// run shares plus the warning that says why. The run then restores nothing and its save
/// lands under a key no later run can match: slow, never wrong, never silent.
pub(super) fn image_or_cold() -> (String, String) {
    match image_identity() {
        Ok(identity) => (identity, String::new()),
        Err(error) => {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let unique = |name: &str| std::env::var(name).unwrap_or_default();
            (
                format!(
                    "cold|{nanos}|{}|{}|{}|{}",
                    std::process::id(),
                    unique("GITHUB_RUN_ID"),
                    unique("GITHUB_RUN_ATTEMPT"),
                    unique("GITHUB_JOB"),
                ),
                format!(
                    "::warning::{}; this run is cold and restores no shared cache\n",
                    escape_data(&error.to_string())
                ),
            )
        }
    }
}

fn image_identity() -> Result<String, CiError> {
    let image_os = std::env::var("ImageOS").unwrap_or_default();
    let explicit = std::env::var_os(OS_RELEASE_OVERRIDE);
    if explicit.is_none() && !cfg!(target_os = "linux") {
        return Ok(format!("{image_os}|{}|", std::env::consts::OS));
    }
    let path = explicit.unwrap_or_else(|| OS_RELEASE.into());
    let os_release = std::fs::read_to_string(&path).map_err(|e| {
        CiError::new(format!(
            "cannot read the OS identity from {}: {e}; without it the cache cannot tell runner images apart",
            Path::new(&path).display()
        ))
    })?;
    let field = |name: &str| {
        os_release
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .map_or_else(String::new, |value| value.trim_matches('"').to_owned())
    };
    Ok(format!(
        "{image_os}|{}|{}",
        field("ID"),
        field("VERSION_ID")
    ))
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, Path, PathBuf};

use crate::error::CiError;

pub const SHARED_DIR_ENV: &str = "VELNOR_MBX_SHARED_DIR";
pub const SHIMS_DIR_NAME: &str = "velnor-mbx-shims";

#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub event_name: &'a str,
    pub git_ref: &'a str,
    pub default_branch: Option<&'a str>,
    pub shared_dir: Option<&'a str>,
    pub runner_temp: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    NotOffered,
    Untrusted,
    Rejected(&'static str),
    Shared { dir: PathBuf, shims: PathBuf },
}

#[must_use]
pub fn trusted_event(event_name: &str, git_ref: &str, default_branch: Option<&str>) -> bool {
    match event_name {
        "push" | "schedule" => default_branch
            .filter(|branch| !branch.is_empty())
            .is_some_and(|branch| git_ref.strip_prefix("refs/heads/") == Some(branch)),
        _ => false,
    }
}

#[must_use]
pub fn select(inputs: &Inputs<'_>) -> Outcome {
    let Some(dir) = inputs.shared_dir.filter(|dir| !dir.is_empty()) else {
        return Outcome::NotOffered;
    };
    if !trusted_event(inputs.event_name, inputs.git_ref, inputs.default_branch) {
        return Outcome::Untrusted;
    }
    match validated(Path::new(dir)) {
        Err(reason) => Outcome::Rejected(reason),
        Ok(dir) => match private_shims(inputs.runner_temp) {
            Ok(shims) => Outcome::Shared { dir, shims },
            Err(reason) => Outcome::Rejected(reason),
        },
    }
}

fn validated(dir: &Path) -> Result<PathBuf, &'static str> {
    if !dir.is_absolute() {
        return Err("not absolute");
    }
    let text = dir.to_str().ok_or("not UTF-8")?;
    if text.chars().any(char::is_control) {
        return Err("control character in path");
    }
    if dir
        .components()
        .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err("not a normal path");
    }
    let metadata = fs::symlink_metadata(dir).map_err(|_| "does not exist")?;
    if metadata.file_type().is_symlink() {
        return Err("is a symlink");
    }
    if !metadata.is_dir() {
        return Err("not a directory");
    }
    if fs::canonicalize(dir).map_err(|_| "unreadable")? != dir {
        return Err("path contains a symlink");
    }
    probe_writable(dir)?;
    Ok(dir.to_path_buf())
}

fn probe_writable(dir: &Path) -> Result<(), &'static str> {
    for _ in 0..8 {
        let probe = dir.join(format!(".velnor-probe-{:016x}", random_u64()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
        {
            Ok(file) => {
                drop(file);
                return fs::remove_file(&probe).map_err(|_| "not writable");
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("not writable"),
        }
    }
    Err("not writable")
}

fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.finish()
}

fn private_shims(runner_temp: &Path) -> Result<PathBuf, &'static str> {
    if !runner_temp.is_absolute() {
        return Err("RUNNER_TEMP is not absolute");
    }
    let shims = runner_temp.join(SHIMS_DIR_NAME);
    match fs::symlink_metadata(&shims) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(shims),
        Ok(_) => Err("shims path is not a directory"),
        Err(_) => fs::DirBuilder::new()
            .mode(0o700)
            .create(&shims)
            .map(|()| shims)
            .map_err(|_| "cannot create the shims directory"),
    }
}

#[must_use]
pub fn env_lines(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Shared { dir, shims } => format!(
            "MBX_CACHE_DIR={}\nMBX_SHIMS_DIR={}\n",
            dir.display(),
            shims.display()
        ),
        _ => String::new(),
    }
}

#[must_use]
pub fn message(outcome: &Outcome) -> String {
    match outcome {
        Outcome::NotOffered => "velnor: MBX uses its per-job store\n".to_owned(),
        Outcome::Untrusted => {
            "velnor: MBX uses its per-job store (untrusted event never writes the shared store)\n"
                .to_owned()
        }
        Outcome::Rejected(reason) => {
            format!("::warning::{SHARED_DIR_ENV} ignored ({reason}); MBX uses its per-job store\n")
        }
        Outcome::Shared { dir, .. } => {
            format!("velnor: MBX uses the shared store {}\n", dir.display())
        }
    }
}

pub fn setup() -> Result<String, CiError> {
    let var = |name: &str| std::env::var(name).ok();
    let runner_temp =
        PathBuf::from(var("RUNNER_TEMP").ok_or_else(|| CiError::new("set RUNNER_TEMP"))?);
    let default_branch = match var("GITHUB_EVENT_PATH") {
        Some(path) => fs::read_to_string(&path)
            .ok()
            .and_then(|text| crate::event::parse_event(&text).ok())
            .and_then(|event| event.default_branch),
        None => None,
    };
    let shared = var(SHARED_DIR_ENV);
    let outcome = select(&Inputs {
        event_name: &var("GITHUB_EVENT_NAME").unwrap_or_default(),
        git_ref: &var("GITHUB_REF").unwrap_or_default(),
        default_branch: default_branch.as_deref(),
        shared_dir: shared.as_deref(),
        runner_temp: &runner_temp,
    });
    let lines = env_lines(&outcome);
    if !lines.is_empty() {
        let file = var("GITHUB_ENV").ok_or_else(|| CiError::new("set GITHUB_ENV"))?;
        fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&file)
            .and_then(|mut handle| handle.write_all(lines.as_bytes()))
            .map_err(|error| CiError::new(format!("cannot write {file}: {error}")))?;
    }
    Ok(message(&outcome))
}

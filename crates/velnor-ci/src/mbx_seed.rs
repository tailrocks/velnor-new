// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::CiError;
use crate::mbx_seed_copy::{Copied, SUBSETS, copy_subset};
use crate::mbx_store::trusted_event;

pub const SEED_DIR_ENV: &str = "VELNOR_MBX_SEED_DIR";
pub const PRIVATE_STORE: &str = "velnor/mbx";

#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub event_name: &'a str,
    pub git_ref: &'a str,
    pub default_branch: Option<&'a str>,
    pub seed_dir: Option<&'a str>,
    pub runner_temp: &'a Path,
    pub owner: u32,
    pub threads: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    NotOffered,
    Trusted,
    Empty,
    Rejected(&'static str),
    Seeded {
        store: PathBuf,
        copied: Copied,
        elapsed: Duration,
    },
}

#[must_use]
pub fn select(inputs: &Inputs<'_>) -> Outcome {
    let Some(dir) = inputs.seed_dir.filter(|dir| !dir.is_empty()) else {
        return Outcome::NotOffered;
    };
    if trusted_event(inputs.event_name, inputs.git_ref, inputs.default_branch) {
        return Outcome::Trusted;
    }
    let seed = match validated(Path::new(dir), inputs.owner) {
        Ok(seed) => seed,
        Err(reason) => return Outcome::Rejected(reason),
    };
    if !SUBSETS
        .iter()
        .any(|name| has_entries(&seed.join("actions").join(name)))
    {
        return Outcome::Empty;
    }
    let store = match private_store(inputs.runner_temp) {
        Ok(store) => store,
        Err(reason) => return Outcome::Rejected(reason),
    };
    let started = Instant::now();
    let copied = copy_subset(
        &seed.join("actions"),
        &store.join("actions"),
        inputs.threads,
    );
    Outcome::Seeded {
        store,
        copied,
        elapsed: started.elapsed(),
    }
}

fn has_entries(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

fn validated(dir: &Path, owner: u32) -> Result<PathBuf, &'static str> {
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
    if metadata.uid() != owner && metadata.uid() != 0 {
        return Err("owned by another user");
    }
    if metadata.mode() & 0o002 != 0 {
        return Err("world-writable");
    }
    Ok(dir.to_path_buf())
}

fn private_store(runner_temp: &Path) -> Result<PathBuf, &'static str> {
    if !runner_temp.is_absolute() {
        return Err("RUNNER_TEMP is not absolute");
    }
    let store = runner_temp.join(PRIVATE_STORE);
    if let Ok(metadata) = fs::symlink_metadata(&store)
        && (metadata.file_type().is_symlink() || !metadata.is_dir())
    {
        return Err("private store path is not a directory");
    }
    fs::create_dir_all(store.join("actions")).map_err(|_| "cannot create the private store")?;
    Ok(store)
}

#[must_use]
pub fn env_lines(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Seeded { store, .. } => format!("MBX_CACHE_DIR={}\n", store.display()),
        _ => String::new(),
    }
}

#[must_use]
pub fn message(outcome: &Outcome) -> String {
    match outcome {
        Outcome::NotOffered => "velnor: MBX seed not offered; MBX builds cold\n".to_owned(),
        Outcome::Trusted => "velnor: MBX seed skipped (trusted event)\n".to_owned(),
        Outcome::Empty => "velnor: MBX seed is empty; MBX builds cold\n".to_owned(),
        Outcome::Rejected(reason) => {
            format!("::warning::{SEED_DIR_ENV} ignored ({reason}); MBX builds cold\n")
        }
        Outcome::Seeded {
            store,
            copied,
            elapsed,
        } => format!(
            "velnor: MBX seeded {} with {} files, {} bytes in {:.2}s ({} already present)\n",
            store.display(),
            copied.files,
            copied.bytes,
            elapsed.as_secs_f64(),
            copied.present
        ),
    }
}

pub fn setup() -> Result<String, CiError> {
    let var = |name: &str| std::env::var(name).ok();
    let runner_temp =
        PathBuf::from(var("RUNNER_TEMP").ok_or_else(|| CiError::new("set RUNNER_TEMP"))?);
    let default_branch = var("GITHUB_EVENT_PATH")
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| crate::event::parse_event(&text).ok())
        .and_then(|event| event.default_branch);
    let seed = var(SEED_DIR_ENV);
    let outcome = select(&Inputs {
        event_name: &var("GITHUB_EVENT_NAME").unwrap_or_default(),
        git_ref: &var("GITHUB_REF").unwrap_or_default(),
        default_branch: default_branch.as_deref(),
        seed_dir: seed.as_deref(),
        runner_temp: &runner_temp,
        owner: owner_of(&runner_temp)?,
        threads: std::thread::available_parallelism().map_or(4, |n| n.get().min(16)),
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

fn owner_of(path: &Path) -> Result<u32, CiError> {
    fs::metadata(path)
        .map(|metadata| metadata.uid())
        .map_err(|error| CiError::new(format!("cannot stat {}: {error}", path.display())))
}

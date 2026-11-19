// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use crate::error::CiError;
use crate::mbx_seed::SEED_DIR_ENV;
use crate::mbx_store::{SHARED_DIR_ENV, trusted_event};

pub const TOOLS_DIR: &str = "tools";
pub const HOME_PATHS: [&str; 2] = [".local/share/mise", ".rustup"];
pub const KEEP_KEYS: usize = 2;
const MARKER: &str = ".complete";
const COPY: &str = "/bin/cp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Verb {
    Restore,
    Save,
}

#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub verb: Verb,
    pub key: &'a str,
    pub event_name: &'a str,
    pub git_ref: &'a str,
    pub default_branch: Option<&'a str>,
    pub shared_dir: Option<&'a str>,
    pub seed_dir: Option<&'a str>,
    pub home: &'a Path,
    pub owner: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    NotOffered,
    BadKey,
    Miss,
    Restored { from: PathBuf },
    Rejected(&'static str),
    Saved { to: PathBuf },
    AlreadySaved,
    Untrusted,
}

impl Outcome {
    #[must_use]
    pub fn hit(&self) -> bool {
        matches!(self, Self::Restored { .. })
    }
}

#[must_use]
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 200
        && !key.starts_with('.')
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[must_use]
pub fn run(inputs: &Inputs<'_>) -> Outcome {
    if !valid_key(inputs.key) {
        return Outcome::BadKey;
    }
    let trusted = trusted_event(inputs.event_name, inputs.git_ref, inputs.default_branch);
    match inputs.verb {
        Verb::Restore => {
            let dir = if trusted {
                inputs.shared_dir
            } else {
                inputs.seed_dir
            };
            restore(inputs, dir)
        }
        Verb::Save if !trusted => Outcome::Untrusted,
        Verb::Save => save(inputs),
    }
}

fn base(dir: Option<&str>, owner: u32) -> Result<PathBuf, Outcome> {
    let Some(dir) = dir.filter(|dir| !dir.is_empty()) else {
        return Err(Outcome::NotOffered);
    };
    let dir = Path::new(dir);
    let reject = |reason| Outcome::Rejected(reason);
    if !dir.is_absolute() || dir.components().any(|part| part.as_os_str() == "..") {
        return Err(reject("not an absolute normal path"));
    }
    let metadata = fs::symlink_metadata(dir).map_err(|_| reject("does not exist"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(reject("not a directory"));
    }
    if metadata.uid() != owner && metadata.uid() != 0 {
        return Err(reject("owned by another user"));
    }
    if metadata.mode() & 0o002 != 0 {
        return Err(reject("world-writable"));
    }
    Ok(dir.to_path_buf())
}

fn complete(entry: &Path, owner: u32) -> bool {
    let sound = |path: &Path| {
        fs::symlink_metadata(path).is_ok_and(|metadata| {
            !metadata.file_type().is_symlink()
                && (metadata.uid() == owner || metadata.uid() == 0)
                && metadata.mode() & 0o002 == 0
        })
    };
    sound(entry)
        && fs::symlink_metadata(entry.join(MARKER)).is_ok_and(|metadata| metadata.is_file())
        && HOME_PATHS
            .iter()
            .all(|path| sound(&entry.join(slot(path))))
}

fn slot(path: &str) -> String {
    path.replace('/', "_")
}

fn copy_tree(from: &Path, to: &Path) -> bool {
    fs::create_dir_all(to).is_ok()
        && Command::new(COPY)
            .arg("-a")
            .arg(format!("{}/.", from.display()))
            .arg(to)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

fn restore(inputs: &Inputs<'_>, dir: Option<&str>) -> Outcome {
    let base = match base(dir, inputs.owner) {
        Ok(base) => base,
        Err(outcome) => return outcome,
    };
    let entry = base.join(TOOLS_DIR).join(inputs.key);
    if !complete(&entry, inputs.owner) {
        return Outcome::Miss;
    }
    for path in HOME_PATHS {
        if !copy_tree(&entry.join(slot(path)), &inputs.home.join(path)) {
            return Outcome::Rejected("a copy failed, so the cache action restores instead");
        }
    }
    Outcome::Restored { from: entry }
}

fn save(inputs: &Inputs<'_>) -> Outcome {
    let base = match base(inputs.shared_dir, inputs.owner) {
        Ok(base) => base,
        Err(outcome) => return outcome,
    };
    let tools = base.join(TOOLS_DIR);
    let entry = tools.join(inputs.key);
    if complete(&entry, inputs.owner) {
        return Outcome::AlreadySaved;
    }
    if fs::create_dir_all(&tools).is_err() {
        return Outcome::Rejected("cannot create the tools directory");
    }
    let staging = tools.join(format!(".staging-{}-{}", inputs.key, std::process::id()));
    fs::remove_dir_all(&staging).ok();
    let copied = HOME_PATHS
        .iter()
        .all(|path| copy_tree(&inputs.home.join(path), &staging.join(slot(path))))
        && fs::File::create(staging.join(MARKER)).is_ok();
    if !copied {
        fs::remove_dir_all(&staging).ok();
        return Outcome::Rejected("the copy to the store failed");
    }
    if fs::symlink_metadata(&entry).is_ok() {
        fs::remove_dir_all(&entry).ok();
    }
    if fs::rename(&staging, &entry).is_err() {
        fs::remove_dir_all(&staging).ok();
        return Outcome::AlreadySaved;
    }
    prune(&tools, inputs.key);
    Outcome::Saved { to: entry }
}

fn prune(tools: &Path, current: &str) {
    let Ok(entries) = fs::read_dir(tools) else {
        return;
    };
    let mut older: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            name != current && !name.to_string_lossy().starts_with('.')
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    older.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in older.into_iter().skip(KEEP_KEYS - 1) {
        fs::remove_dir_all(path).ok();
    }
}

#[must_use]
pub fn message(outcome: &Outcome) -> String {
    match outcome {
        Outcome::NotOffered => "velnor: no node-local tools store offered\n".to_owned(),
        Outcome::BadKey => "::warning::the tools key is not a plain name; the node store is skipped\n".to_owned(),
        Outcome::Miss => "velnor: the node-local tools store has no entry for this key\n".to_owned(),
        Outcome::Restored { from } => {
            format!("velnor: tools restored from the node store {}\n", from.display())
        }
        Outcome::Rejected(reason) => {
            format!("::warning::node-local tools store ignored ({reason})\n")
        }
        Outcome::Saved { to } => format!("velnor: tools saved to the node store {}\n", to.display()),
        Outcome::AlreadySaved => "velnor: the node store already holds this key\n".to_owned(),
        Outcome::Untrusted => "velnor: tools are saved to the node store on trusted events only\n".to_owned(),
    }
}

pub fn setup(verb: Verb, key: &str) -> Result<String, CiError> {
    let var = |name: &str| std::env::var(name).ok();
    let home = PathBuf::from(var("HOME").ok_or_else(|| CiError::new("set HOME"))?);
    let default_branch = var("GITHUB_EVENT_PATH")
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| crate::event::parse_event(&text).ok())
        .and_then(|event| event.default_branch);
    let (shared, seed) = (var(SHARED_DIR_ENV), var(SEED_DIR_ENV));
    let outcome = run(&Inputs {
        verb,
        key,
        event_name: &var("GITHUB_EVENT_NAME").unwrap_or_default(),
        git_ref: &var("GITHUB_REF").unwrap_or_default(),
        default_branch: default_branch.as_deref(),
        shared_dir: shared.as_deref(),
        seed_dir: seed.as_deref(),
        home: &home,
        owner: fs::metadata(&home)
            .map(|metadata| metadata.uid())
            .map_err(|error| CiError::new(format!("cannot stat {}: {error}", home.display())))?,
    });
    if verb == Verb::Restore
        && let Some(file) = var("GITHUB_OUTPUT")
    {
        fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&file)
            .and_then(|mut handle| writeln!(handle, "hit={}", outcome.hit()))
            .map_err(|error| CiError::new(format!("cannot write {file}: {error}")))?;
    }
    Ok(message(&outcome))
}

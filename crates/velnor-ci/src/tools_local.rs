// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::error::CiError;
use crate::mbx_seed::SEED_DIR_ENV;
use crate::mbx_store::{SHARED_DIR_ENV, trusted_event};

mod store;
use store::{restore, save};

pub const TOOLS_DIR: &str = "tools";
pub const HOME_PATHS: [&str; 2] = [".local/share/mise", ".rustup"];
pub const KEEP_KEYS: usize = 2;

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
    Busy,
    Failed(&'static str),
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
            restore(inputs, dir, !trusted)
        }
        Verb::Save if !trusted => Outcome::Untrusted,
        Verb::Save => save(inputs),
    }
}

/// The node a tools line came from. Kubernetes sets `NODE_NAME` when the pod asks for it (the
/// node, for a misses-by-node count); the hostname is the pod name otherwise. Only plain name
/// characters survive, so the label can never open a workflow command.
#[must_use]
pub fn node_label(node_name: Option<&str>, hostname: Option<&str>) -> String {
    [node_name, hostname]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|name| !name.is_empty())
        .unwrap_or("unknown")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(253)
        .collect()
}

fn this_node() -> String {
    let hostname = fs::read_to_string("/etc/hostname")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok());
    node_label(
        std::env::var("NODE_NAME").ok().as_deref(),
        hostname.as_deref(),
    )
}

#[must_use]
pub fn message(outcome: &Outcome, node: &str) -> String {
    format!("{} node={node}\n", describe(outcome))
}

fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::NotOffered => "velnor: no node-local tools store offered".to_owned(),
        Outcome::BadKey => {
            "::warning::the tools key is not a plain name; the node store is skipped".to_owned()
        }
        Outcome::Miss => "velnor: the node-local tools store has no entry for this key".to_owned(),
        Outcome::Restored { from } => {
            format!(
                "velnor: tools restored from the node store {}",
                from.display()
            )
        }
        Outcome::Rejected(reason) => {
            format!("::warning::node-local tools store ignored ({reason})")
        }
        Outcome::Saved { to } => {
            format!("velnor: tools saved to the node store {}", to.display())
        }
        Outcome::AlreadySaved => "velnor: the node store already holds this key".to_owned(),
        Outcome::Busy => "velnor: another job is saving this key to the node store".to_owned(),
        Outcome::Failed(reason) => {
            format!("::error::tools were not saved to the node store ({reason})")
        }
        Outcome::Untrusted => {
            "velnor: tools are saved to the node store on trusted events only".to_owned()
        }
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
    let node = this_node();
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
    finish(verb, &outcome, &node)
}

/// The text the step prints, or an error when a save that should have happened did not, so that
/// the workflow can tell a failed save from a skipped one.
pub fn finish(verb: Verb, outcome: &Outcome, node: &str) -> Result<String, CiError> {
    match outcome {
        Outcome::Failed(reason) if verb == Verb::Save => Err(CiError::new(format!(
            "tools were not saved to the node store ({reason}) node={node}"
        ))),
        _ => Ok(message(outcome, node)),
    }
}

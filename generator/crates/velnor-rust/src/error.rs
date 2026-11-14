// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use velnor_model::{PathGlob, RepoPath};

use crate::config::CONFIG_PATH;
use crate::lock::LockError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectError {
    Invalid { path: RepoPath, reason: Reason },
    MissingLockfile { path: RepoPath, manifest: RepoPath },
    MiseRequired { path: RepoPath },
    NoRustRoot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    NotUtf8,
    Syntax(String),
    UnknownKey(String),
    MissingTable(&'static str),
    MissingKey(&'static str),
    InvalidValue {
        key: &'static str,
        value: String,
    },
    OutOfRange {
        key: &'static str,
        value: u16,
        min: u16,
        max: u16,
    },
    UnmatchedExclude(PathGlob),
    HelperLock(LockError),
    InvalidPatch(String),
    OverlappingRules {
        non_input: PathGlob,
        input: PathGlob,
    },
    UnknownInputPackage(String),
    LockLine(usize),
    UnknownLockedPackage(String),
}

impl DetectError {
    pub(crate) fn invalid(path: &RepoPath, reason: Reason) -> Self {
        Self::Invalid {
            path: path.clone(),
            reason,
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 => f.write_str("is not UTF-8"),
            Self::Syntax(message) => f.write_str(message.trim_end()),
            Self::UnknownKey(key) => write!(f, "unknown key {key}"),
            Self::MissingTable(table) => write!(f, "missing table [{table}]"),
            Self::MissingKey(key) => write!(f, "missing key {key}"),
            Self::InvalidValue { key, value } => write!(f, "invalid value {value:?} for {key}"),
            Self::OutOfRange {
                key,
                value,
                min,
                max,
            } => write!(f, "{key} must be between {min} and {max}, got {value}"),
            Self::UnmatchedExclude(glob) => {
                write!(f, "exclude entry {glob} matches no tracked Cargo.toml")
            }
            Self::HelperLock(error) => error.fmt(f),
            Self::OverlappingRules { non_input, input } => write!(
                f,
                "non_inputs entry {non_input} overlaps inputs entry {input}; a path cannot select nothing and select packages"
            ),
            Self::UnknownInputPackage(package) => write!(
                f,
                "inputs names {package}, which is not a package of this repository"
            ),
            Self::LockLine(line) => write!(f, "line {line} is not `path<TAB>package`"),
            Self::UnknownLockedPackage(package) => write!(
                f,
                "lists {package}, which is not a package of this repository; run `velnor-ci inputs audit --write` after a build of all targets"
            ),
            Self::InvalidPatch(key) => {
                write!(
                    f,
                    "{key} must be a table whose path is a folder in the repository"
                )
            }
        }
    }
}

impl fmt::Display for DetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { path, reason } => write!(f, "{path}: {reason}"),
            Self::MissingLockfile { path, manifest } => write!(
                f,
                "{path} is not committed; commit it, or add the directory of {manifest} to exclude in {CONFIG_PATH}"
            ),
            Self::MiseRequired { path } => write!(
                f,
                "a mise configuration is required; commit {path} with the tool versions the project uses"
            ),
            Self::NoRustRoot => f.write_str("no Cargo workspace found outside the excluded paths"),
        }
    }
}

impl std::error::Error for DetectError {}

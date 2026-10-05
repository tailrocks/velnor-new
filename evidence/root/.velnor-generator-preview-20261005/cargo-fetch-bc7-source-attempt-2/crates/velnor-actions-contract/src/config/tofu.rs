//! Minimal `[stacks.tofu]` roots configuration (schema 1, strict).
//!
//! T09 owns the shape only: a REQUIRED non-empty sorted duplicate-free
//! root list plus lexical grammar. Filesystem qualification (canonical
//! containment, effective-config presence) lives in the tofu adapter,
//! which sees the repository root and file index.

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Tofu stack options: strict minimal roots list, no other v1 keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TofuStackConfig {
    /// Configured validation roots; `.` names the repository root.
    pub roots: Vec<Utf8RepoRelDir>,
}

/// Repository-relative tofu root directory (validated at load).
///
/// The raw configured spelling; [`Utf8RepoRelDir::unit_prefix`] maps
/// `.` to the empty detector prefix. Every other valid spelling is
/// already canonical: normalization is rejected, never applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Utf8RepoRelDir(String);

/// One lexical root rejection with its machine-readable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootProblem {
    /// Empty entry.
    Empty,
    /// Leading `/`.
    Absolute,
    /// Windows drive prefix (`C:`).
    DriveLetter,
    /// Backslash separator.
    Backslash,
    /// Control byte (never echoes the value).
    ControlByte,
    /// `.` segment inside a longer path.
    DotSegment,
    /// `..` segment.
    DotDot,
    /// Empty segment (`//`, leading/trailing `/`).
    EmptySegment,
    /// Reserved `root` spelling (collides with the repo-root key).
    ReservedRoot,
}

impl RootProblem {
    /// Machine-readable code, echoing printable values only.
    fn code(&self, value: &str) -> String {
        match self {
            Self::Empty => "empty_root".to_owned(),
            Self::Absolute => format!("absolute_root:{value}"),
            Self::DriveLetter => format!("drive_letter:{value}"),
            Self::Backslash => format!("backslash:{value}"),
            Self::ControlByte => "control_byte".to_owned(),
            Self::DotSegment => format!("dot_segment:{value}"),
            Self::DotDot => format!("dotdot_segment:{value}"),
            Self::EmptySegment => format!("empty_segment:{value}"),
            Self::ReservedRoot => "reserved_root_key".to_owned(),
        }
    }
}

impl Utf8RepoRelDir {
    /// Wrap a raw configured spelling (validation runs at load).
    #[must_use]
    pub fn from_raw(raw: String) -> Self {
        Self(raw)
    }

    /// Borrow the configured spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Detector prefix: empty for `.`, the spelling otherwise.
    ///
    /// Callers validate first; unvalidated spellings still map
    /// deterministically (`.` folds, everything else passes through).
    #[must_use]
    pub fn unit_prefix(&self) -> &str {
        if self.0 == "." { "" } else { self.0.as_str() }
    }

    /// Check one spelling against the strict root grammar.
    /// # Errors
    pub fn parse(value: &str) -> Result<Self, RootProblem> {
        check_root(value)?;
        Ok(Self(value.to_owned()))
    }
}

/// Check one root spelling; the single grammar owner.
fn check_root(value: &str) -> Result<(), RootProblem> {
    if value.is_empty() {
        return Err(RootProblem::Empty);
    }
    if value.chars().any(char::is_control) {
        return Err(RootProblem::ControlByte);
    }
    if value.starts_with('/') {
        return Err(RootProblem::Absolute);
    }
    if is_drive_prefixed(value) {
        return Err(RootProblem::DriveLetter);
    }
    if value.contains('\\') {
        return Err(RootProblem::Backslash);
    }
    if value == "." {
        return Ok(());
    }
    if value == "root" {
        return Err(RootProblem::ReservedRoot);
    }
    for segment in value.split('/') {
        if segment.is_empty() {
            return Err(RootProblem::EmptySegment);
        }
        if segment == "." {
            return Err(RootProblem::DotSegment);
        }
        if segment == ".." {
            return Err(RootProblem::DotDot);
        }
    }
    Ok(())
}

/// True for a Windows drive prefix (`C:`/`c:` plus more path).
fn is_drive_prefixed(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

impl TofuStackConfig {
    /// Validate roots: non-empty, sorted, duplicate-free, grammatical.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.roots.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.tofu.roots",
                "empty_roots",
            ));
        }
        let mut sorted: Vec<&str> = self.roots.iter().map(Utf8RepoRelDir::as_str).collect();
        sorted.sort_unstable();
        let ordered: Vec<&str> = self.roots.iter().map(Utf8RepoRelDir::as_str).collect();
        if sorted != ordered {
            return Err(ContractError::config(
                file,
                "stacks.tofu.roots",
                "must_be_sorted",
            ));
        }
        let unique: BTreeSet<&str> = ordered.iter().copied().collect();
        if unique.len() != ordered.len() {
            return Err(ContractError::config(
                file,
                "stacks.tofu.roots",
                "duplicate_root",
            ));
        }
        for root in &self.roots {
            if let Err(problem) = check_root(root.as_str()) {
                return Err(ContractError::config(
                    file,
                    "stacks.tofu.roots",
                    problem.code(root.as_str()),
                ));
            }
        }
        Ok(())
    }
}

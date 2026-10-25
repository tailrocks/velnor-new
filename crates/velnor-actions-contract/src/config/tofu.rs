use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TofuStackConfig {
    pub roots: Vec<Utf8RepoRelDir>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Utf8RepoRelDir(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootProblem {
    Empty,

    Absolute,

    DriveLetter,

    Backslash,

    ControlByte,

    DotSegment,

    DotDot,

    EmptySegment,
}

impl RootProblem {
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
        }
    }
}

impl Utf8RepoRelDir {
    #[must_use]
    pub fn from_raw(raw: String) -> Self {
        Self(raw)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn unit_prefix(&self) -> &str {
        if self.0 == "." { "" } else { self.0.as_str() }
    }

    pub fn parse(value: &str) -> Result<Self, RootProblem> {
        check_root(value)?;
        Ok(Self(value.to_owned()))
    }
}

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

fn is_drive_prefixed(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

impl TofuStackConfig {
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

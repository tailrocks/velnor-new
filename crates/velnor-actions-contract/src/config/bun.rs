use super::tofu::Utf8RepoRelDir;
use crate::errors::ContractError;
use crate::workflow::timeout::JobTimeout;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_BUN_TIMEOUT_MINUTES: u32 = 360;

pub const DEFAULT_BUN_SCRIPTS: [&str; 4] = ["typecheck", "lint", "test", "build"];

pub const BUN_JOB_PREFIX: &str = "bun-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BunStackConfig {
    pub roots: Vec<Utf8RepoRelDir>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u32>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<String>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub members: BTreeMap<String, BunMemberConfig>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<BunInputRule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BunMemberConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub browsers: Vec<BunBrowser>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u32>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub suites: BTreeMap<String, BunSuiteConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BunBrowser {
    Chromium,

    Firefox,

    Webkit,
}

impl BunBrowser {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chromium => "chromium",
            Self::Firefox => "firefox",
            Self::Webkit => "webkit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BunInputRule {
    pub globs: Vec<String>,

    pub members: Vec<String>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub complete: bool,
}

#[must_use]
pub fn is_valid_script_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
}

#[must_use]
pub fn is_valid_member_name(name: &str) -> bool {
    let (scope, base) = match name.strip_prefix('@') {
        Some(rest) => match rest.split_once('/') {
            Some((scope, base)) => (Some(scope), base),
            None => return false,
        },
        None => (None, name),
    };
    let part_ok = |part: &str| {
        !part.is_empty()
            && part.len() <= 64
            && part
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric())
            && part.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
            })
    };
    scope.is_none_or(part_ok) && part_ok(base)
}

impl BunStackConfig {
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        self.validate_roots(file)?;
        if let Some(minutes) = self.timeout_minutes
            && !(1..=MAX_BUN_TIMEOUT_MINUTES).contains(&minutes)
        {
            return Err(ContractError::config(
                file,
                "stacks.bun.timeout_minutes",
                format!("must_be_between_1_and_{MAX_BUN_TIMEOUT_MINUTES}"),
            ));
        }
        validate_scripts(&self.scripts, "stacks.bun.scripts", file)?;
        for (name, member) in &self.members {
            if !is_valid_member_name(name) {
                return Err(ContractError::config(
                    file,
                    "stacks.bun.members",
                    format!("bad_member_name:{name}"),
                ));
            }
            if let Some(scripts) = &member.scripts {
                validate_scripts(scripts, "stacks.bun.members.scripts", file)?;
            }
            validate_browsers(&member.browsers, file)?;
            if let Some(minutes) = member.timeout_minutes {
                JobTimeout::check_config(minutes, file, "stacks.bun.members.timeout_minutes")?;
            }
            suites::validate_suites(&member.suites, file)?;
        }
        self.validate_inputs(file)
    }

    fn validate_roots(&self, file: &str) -> Result<(), ContractError> {
        let roots: Vec<&str> = self.roots.iter().map(Utf8RepoRelDir::as_str).collect();
        if roots.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.bun.roots",
                "empty_roots",
            ));
        }
        let mut sorted = roots.clone();
        sorted.sort_unstable();
        if sorted != roots {
            return Err(ContractError::config(
                file,
                "stacks.bun.roots",
                "must_be_sorted",
            ));
        }
        if sorted.iter().collect::<BTreeSet<_>>().len() != roots.len() {
            return Err(ContractError::config(
                file,
                "stacks.bun.roots",
                "duplicate_root",
            ));
        }
        for root in &roots {
            if let Err(problem) = Utf8RepoRelDir::parse(root) {
                return Err(ContractError::config(
                    file,
                    "stacks.bun.roots",
                    format!("bad_root:{problem:?}"),
                ));
            }
        }
        Ok(())
    }

    fn validate_inputs(&self, file: &str) -> Result<(), ContractError> {
        for rule in &self.inputs {
            if rule.globs.is_empty() || rule.members.is_empty() {
                return Err(ContractError::config(
                    file,
                    "stacks.bun.inputs",
                    "empty_globs_or_members",
                ));
            }
            for glob in &rule.globs {
                if crate::discover::validate_pattern(glob).is_err() {
                    return Err(ContractError::config(
                        file,
                        "stacks.bun.inputs.globs",
                        format!("bad_glob:{glob}"),
                    ));
                }
            }
            let mut seen = BTreeSet::new();
            for member in &rule.members {
                if !is_valid_member_name(member) || !seen.insert(member.as_str()) {
                    return Err(ContractError::config(
                        file,
                        "stacks.bun.inputs.members",
                        format!("bad_or_duplicate_member:{member}"),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn validate_scripts(scripts: &[String], key: &str, file: &str) -> Result<(), ContractError> {
    let mut seen = BTreeSet::new();
    for script in scripts {
        if !is_valid_script_name(script) {
            return Err(ContractError::config(
                file,
                key,
                format!("bad_script_name:{script}"),
            ));
        }
        if !seen.insert(script.as_str()) {
            return Err(ContractError::config(
                file,
                key,
                format!("duplicate_script:{script}"),
            ));
        }
    }
    Ok(())
}

fn validate_browsers(browsers: &[BunBrowser], file: &str) -> Result<(), ContractError> {
    let mut sorted = browsers.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted != browsers {
        return Err(ContractError::config(
            file,
            "stacks.bun.members.browsers",
            "must_be_sorted_and_unique",
        ));
    }
    Ok(())
}

#[path = "bun_suites.rs"]
mod suites;
pub use suites::*;

//! `[stacks.bun]`: Bun workspace roots, per-member scripts and browsers.
//!
//! Hard decision 6: every workspace member owns one generated job whose
//! script steps run in parallel. The table below is strict (unknown keys are
//! errors) and carries no shell, argv, YAML or `uses:` field: scripts are
//! `package.json` script names, never command text.

use super::tofu::Utf8RepoRelDir;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Largest accepted job timeout, matching the other stacks.
pub const MAX_BUN_TIMEOUT_MINUTES: u32 = 360;
/// Scripts a member runs when neither `[stacks.bun]` nor the member names any.
pub const DEFAULT_BUN_SCRIPTS: [&str; 4] = ["typecheck", "lint", "test", "build"];
/// Job id prefix of every generated Bun member job.
pub const BUN_JOB_PREFIX: &str = "bun-";

/// Bun stack options (`[stacks.bun]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BunStackConfig {
    /// Workspace roots: sorted, duplicate-free; each holds `package.json` and `bun.lock`.
    pub roots: Vec<Utf8RepoRelDir>,
    /// Member job timeout in minutes; defaults to `workflow.crate_timeout_minutes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u32>,
    /// Scripts every member runs when it has them; defaults to [`DEFAULT_BUN_SCRIPTS`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<String>,
    /// Per-member overrides keyed by the member's `package.json` name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub members: BTreeMap<String, BunMemberConfig>,
    /// Declared reads outside a member's directory (`[[stacks.bun.inputs]]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<BunInputRule>,
}

/// Per-member options (`[stacks.bun.members."<name>"]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BunMemberConfig {
    /// Scripts replacing the stack default; every name must exist in the member's `package.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<Vec<String>>,
    /// Playwright browsers the member's scripts need, installed by Velnor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub browsers: Vec<BunBrowser>,
}

/// Closed set of Playwright browsers Velnor provisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BunBrowser {
    /// Playwright Chromium.
    Chromium,
    /// Playwright Firefox.
    Firefox,
    /// Playwright `WebKit`.
    Webkit,
}

impl BunBrowser {
    /// Name Playwright's `install` command and browser registry use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chromium => "chromium",
            Self::Firefox => "firefox",
            Self::Webkit => "webkit",
        }
    }
}

/// Reads a member performs outside its own directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BunInputRule {
    /// Repository-relative globs (`**`, `*`, `?`, `[...]`, `{a,b}`).
    pub globs: Vec<String>,
    /// Member names (`package.json` `name`) consuming the matched paths.
    pub members: Vec<String>,
    /// The named members' rules list every path they read outside their directories.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub complete: bool,
}

/// True for a script name that can never carry shell or YAML syntax.
#[must_use]
pub fn is_valid_script_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
}

/// True for an npm package name Velnor accepts as a member key.
///
/// Optional `@scope/` plus lowercase alphanumerics, `-`, `_` and `.`.
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
    /// Validate roots, scripts, members and declared inputs.
    /// # Errors
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

#[cfg(test)]
#[path = "bun_tests.rs"]
mod tests;

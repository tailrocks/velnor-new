//! `[stacks.ansible]`: Ansible roots, hash-pinned Python tooling and offline collections.
//!
//! The table is strict (unknown keys are errors) and carries no shell, argv,
//! YAML or `uses:` field. Checks are a closed enum, the Python requirements
//! file must be fully hash pinned (verified by the adapter) and every
//! collection is an exact tarball with a declared SHA-256 that Velnor fetches
//! and installs offline. Host rollout is out of scope (hard decision 9).

use super::tofu::Utf8RepoRelDir;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Largest accepted job timeout, matching the other stacks.
pub(super) const MAX_ANSIBLE_TIMEOUT_MINUTES: u32 = 360;
/// Job id prefix of every generated Ansible root job.
pub const ANSIBLE_JOB_PREFIX: &str = "ansible-";

/// Ansible stack options (`[stacks.ansible]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnsibleStackConfig {
    /// Roots holding the playbooks: sorted, duplicate-free.
    pub roots: Vec<Utf8RepoRelDir>,
    /// Repository-relative `requirements.txt`; every requirement hash pinned.
    pub python_requirements: String,
    /// Repository-relative inventory file used by `--syntax-check`.
    pub inventory: String,
    /// Checks the job runs; defaults to `syntax` then `lint`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<AnsibleCheck>,
    /// `ansible-lint` profile; defaults to [`LintProfile::Production`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint_profile: Option<LintProfile>,
    /// Exact collection tarballs, sorted by name; installed offline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<AnsibleCollection>,
    /// Job timeout in minutes; defaults to `workflow.crate_timeout_minutes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u32>,
}

/// Closed set of Ansible checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnsibleCheck {
    /// `ansible-playbook --syntax-check` over every top-level playbook.
    Syntax,
    /// `ansible-lint --offline`.
    Lint,
}

/// The `ansible-lint` profiles, loosest to strictest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintProfile {
    /// `min`.
    Min,
    /// `basic`.
    Basic,
    /// `moderate`.
    Moderate,
    /// `safety`.
    Safety,
    /// `shared`.
    Shared,
    /// `production`.
    Production,
}

impl LintProfile {
    /// Name `ansible-lint --profile` takes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Min => "min",
            Self::Basic => "basic",
            Self::Moderate => "moderate",
            Self::Safety => "safety",
            Self::Shared => "shared",
            Self::Production => "production",
        }
    }
}

/// One exact collection tarball (`[[stacks.ansible.collections]]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnsibleCollection {
    /// `<namespace>.<name>`.
    pub name: String,
    /// Exact `X.Y.Z`.
    pub version: String,
    /// Lower-case hex SHA-256 of the tarball.
    pub sha256: String,
}

impl AnsibleStackConfig {
    /// Checks the job runs: the declared set, or `syntax` and `lint`.
    #[must_use]
    pub fn effective_checks(&self) -> Vec<AnsibleCheck> {
        if self.checks.is_empty() {
            vec![AnsibleCheck::Syntax, AnsibleCheck::Lint]
        } else {
            self.checks.clone()
        }
    }

    /// Profile `ansible-lint` runs with.
    #[must_use]
    pub fn effective_lint_profile(&self) -> LintProfile {
        self.lint_profile.unwrap_or(LintProfile::Production)
    }

    /// Validate roots, paths, checks and collections.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        self.validate_roots(file)?;
        for (key, path) in [
            (
                "stacks.ansible.python_requirements",
                &self.python_requirements,
            ),
            ("stacks.ansible.inventory", &self.inventory),
        ] {
            if !is_valid_file_path(path) {
                return Err(ContractError::config(file, key, "bad_file_path"));
            }
        }
        if let Some(minutes) = self.timeout_minutes
            && !(1..=MAX_ANSIBLE_TIMEOUT_MINUTES).contains(&minutes)
        {
            return Err(ContractError::config(
                file,
                "stacks.ansible.timeout_minutes",
                format!("must_be_between_1_and_{MAX_ANSIBLE_TIMEOUT_MINUTES}"),
            ));
        }
        let mut checks = self.checks.clone();
        checks.sort_unstable();
        checks.dedup();
        if checks != self.checks {
            return Err(ContractError::config(
                file,
                "stacks.ansible.checks",
                "must_be_sorted_and_unique",
            ));
        }
        if self.lint_profile.is_some() && !self.effective_checks().contains(&AnsibleCheck::Lint) {
            return Err(ContractError::config(
                file,
                "stacks.ansible.lint_profile",
                "requires_the_lint_check",
            ));
        }
        self.validate_collections(file)
    }

    fn validate_roots(&self, file: &str) -> Result<(), ContractError> {
        let roots: Vec<&str> = self.roots.iter().map(Utf8RepoRelDir::as_str).collect();
        let key = "stacks.ansible.roots";
        if roots.is_empty() {
            return Err(ContractError::config(file, key, "empty_roots"));
        }
        let mut sorted = roots.clone();
        sorted.sort_unstable();
        if sorted != roots {
            return Err(ContractError::config(file, key, "must_be_sorted"));
        }
        if sorted.iter().collect::<BTreeSet<_>>().len() != roots.len() {
            return Err(ContractError::config(file, key, "duplicate_root"));
        }
        for root in &roots {
            if let Err(problem) = Utf8RepoRelDir::parse(root) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("bad_root:{problem:?}"),
                ));
            }
        }
        Ok(())
    }

    fn validate_collections(&self, file: &str) -> Result<(), ContractError> {
        let key = "stacks.ansible.collections";
        let names: Vec<&str> = self.collections.iter().map(|c| c.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted != names {
            return Err(ContractError::config(
                file,
                key,
                "must_be_sorted_and_unique",
            ));
        }
        for collection in &self.collections {
            if !is_valid_collection_name(&collection.name) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("bad_collection_name:{}", collection.name),
                ));
            }
            if !is_exact_version(&collection.version) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("bad_collection_version:{}", collection.name),
                ));
            }
            if !is_sha256(&collection.sha256) {
                return Err(ContractError::config(
                    file,
                    key,
                    format!("bad_collection_sha256:{}", collection.name),
                ));
            }
        }
        Ok(())
    }
}

/// True for `<namespace>.<name>` over lower-case alphanumerics and `_`.
#[must_use]
pub(super) fn is_valid_collection_name(name: &str) -> bool {
    let mut parts = name.split('.');
    let part_ok = |part: &str| {
        !part.is_empty()
            && part.len() <= 64
            && part.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    };
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(namespace), Some(base), None) if part_ok(namespace) && part_ok(base)
    )
}

/// True for an exact `X.Y.Z` of canonical decimal components.
#[must_use]
pub(super) fn is_exact_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
}

/// True for 64 lower-case hex digits.
#[must_use]
pub(super) fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// True for a repository-relative file path made of safe segments.
#[must_use]
pub(super) fn is_valid_file_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        })
}

#[cfg(test)]
#[path = "ansible_tests.rs"]
mod tests;

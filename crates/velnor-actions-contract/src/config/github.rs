//! `[github]`: policy for `.github` entries the generator does not emit.
//!
//! Generation preserves every `.github` entry it does not own, so a stray
//! hand-written file passes the `Check generated files` drift step. This table
//! opts a repository into reporting those entries.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

/// How unmanaged `.github` entries are treated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnmanagedPolicy {
    /// Preserve silently (default; historical behavior).
    #[default]
    Allow,
    /// Print a notice per generation and keep succeeding.
    Warn,
    /// Fail generation, and with it the drift step.
    Deny,
}

impl UnmanagedPolicy {
    /// True when the policy is the silent default.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// `[github]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubConfig {
    /// Treatment of `.github` entries the generator would not emit.
    #[serde(default, skip_serializing_if = "UnmanagedPolicy::is_default")]
    pub unmanaged: UnmanagedPolicy,
    /// Exempt repository-relative paths: an exact file, or a directory that
    /// exempts everything below it (for example `.github/CODEOWNERS`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
}

impl GithubConfig {
    /// True when all fields match their defaults.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.unmanaged.is_default() && self.allow.is_empty()
    }

    /// True when `path` (repository-relative, `.github/...`) is exempt.
    #[must_use]
    pub fn is_allowed(&self, path: &str) -> bool {
        self.allow.iter().any(|entry| {
            path == entry
                || path
                    .strip_prefix(entry.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }

    /// Validate allowlist entries: normalized `.github/<path>`, no duplicates,
    /// and nothing under `.github/workflows/` (use `preserve_workflows`).
    ///
    /// # Errors
    ///
    /// Returns a config error naming `github.allow` and the bad entry.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut seen = std::collections::BTreeSet::new();
        for entry in &self.allow {
            let bad = |problem: &str| {
                ContractError::config(file, "github.allow", format!("{problem}:{entry}"))
            };
            let Some(rest) = entry.strip_prefix(".github/") else {
                return Err(bad("allow_outside_github"));
            };
            if rest.is_empty()
                || rest
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || rest.contains('\\')
            {
                return Err(bad("allow_path_not_normalized"));
            }
            if rest == "workflows" || rest.starts_with("workflows/") {
                return Err(bad("allow_workflow_use_preserve_workflows"));
            }
            if !seen.insert(entry.as_str()) {
                return Err(bad("duplicate_allow"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{GithubConfig, UnmanagedPolicy};

    const FILE: &str = ".velnor/config.toml";

    fn config(allow: &[&str]) -> GithubConfig {
        GithubConfig {
            unmanaged: UnmanagedPolicy::Deny,
            allow: allow.iter().map(|entry| (*entry).to_owned()).collect(),
        }
    }

    fn problem(allow: &[&str]) -> String {
        config(allow)
            .validate(FILE)
            .expect_err("invalid allow")
            .to_string()
    }

    #[test]
    fn default_is_allow_and_empty() {
        let default = GithubConfig::default();
        assert!(default.is_default());
        assert_eq!(default.unmanaged, UnmanagedPolicy::Allow);
        assert_eq!(default.validate(FILE), Ok(()));
    }

    #[test]
    fn rejects_bad_allow_entries() {
        assert!(problem(&["CODEOWNERS"]).contains("allow_outside_github"));
        assert!(problem(&[".github/"]).contains("allow_path_not_normalized"));
        assert!(problem(&[".github/a/../b"]).contains("allow_path_not_normalized"));
        assert!(problem(&[".github/a//b"]).contains("allow_path_not_normalized"));
        assert!(problem(&[".github/workflows/x.yml"]).contains("use_preserve_workflows"));
        assert!(problem(&[".github/CODEOWNERS", ".github/CODEOWNERS"]).contains("duplicate_allow"));
    }

    #[test]
    fn allow_matches_file_and_directory_children_only() {
        let config = config(&[".github/CODEOWNERS", ".github/ISSUE_TEMPLATE"]);
        assert_eq!(config.validate(FILE), Ok(()));
        assert!(config.is_allowed(".github/CODEOWNERS"));
        assert!(config.is_allowed(".github/ISSUE_TEMPLATE/bug.md"));
        assert!(!config.is_allowed(".github/CODEOWNERS2"));
        assert!(!config.is_allowed(".github/ISSUE_TEMPLATES/bug.md"));
    }
}

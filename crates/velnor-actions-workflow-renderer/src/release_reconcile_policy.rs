//! Immutable approval for package evidence and publication reconciliation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::config::ReleaseAuthentication;

use crate::{RenderError, release_spec::BootstrapPlan};

/// Complete release approval; helper environment data must equal this value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseReconcilePolicy {
    /// Fixed evidence schema.
    pub schema: u32,
    /// Exact repository authority.
    pub repository: String,
    /// Exact registry authority.
    pub registry: String,
    /// Immutable approved source.
    pub source_sha: String,
    /// Exact selected versions.
    pub packages: BTreeMap<String, String>,
    /// Exact approved owner identities, including user and team namespaces.
    pub owners: BTreeMap<String, Vec<String>>,
    /// Exact package release tags.
    pub tags: BTreeMap<String, String>,
    /// Exclusive publication authentication mode.
    pub authentication: ReleaseAuthentication,
    /// Exact generator and executable versions.
    pub tools: BTreeMap<String, String>,
    /// Exact source-bound intent identity.
    pub intent_id: String,
}

impl ReleaseReconcilePolicy {
    /// Validate complete approval against the independently bound source plan.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for invalid or conflicting approval fields.
    pub fn validate(&self, plan: &BootstrapPlan) -> Result<(), RenderError> {
        let expected_auth = if plan.version.is_some() {
            ReleaseAuthentication::BootstrapToken
        } else {
            ReleaseAuthentication::TrustedPublishing
        };
        if self.schema != 1
            || self.repository != plan.repository
            || self.registry != "crates-io"
            || self.registry != plan.registry
            || self.source_sha != plan.source_sha
            || self.packages != plan.packages
            || self.authentication != expected_auth
            || self.intent_id != format!("release-{}", plan.source_sha)
            || self.owners.keys().ne(self.packages.keys())
            || self.tags.keys().ne(self.packages.keys())
        {
            return Err(invalid("release_reconcile_approval_mismatch"));
        }
        for owners in self.owners.values() {
            if owners.is_empty()
                || owners.windows(2).any(|pair| pair[0] >= pair[1])
                || owners.iter().any(|owner| !valid_owner(owner))
            {
                return Err(invalid("release_reconcile_owner_approval"));
            }
        }
        if self.tags.values().any(|tag| !valid_tag(tag)) {
            return Err(invalid("release_reconcile_tag_approval"));
        }
        let expected = ["generator", "gh", "python", "release-plz", "rust"];
        if self.tools.keys().map(String::as_str).ne(expected)
            || self.tools.values().any(|pin| !numeric_pin(pin))
        {
            return Err(invalid("release_reconcile_tool_approval"));
        }
        Ok(())
    }

    /// Serialize the exact immutable approval for helper environment binding.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] when serialization fails.
    pub fn serialized(&self) -> Result<String, RenderError> {
        serde_json::to_string(self).map_err(|error| invalid(&error.to_string()))
    }
}

fn valid_owner(owner: &str) -> bool {
    let Some((namespace, id)) = owner.split_once(':') else {
        return false;
    };
    matches!(namespace, "user" | "team")
        && !id.starts_with('0')
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && id.parse::<u64>().is_ok_and(|value| value > 0)
}

fn numeric_pin(pin: &str) -> bool {
    let parts: Vec<_> = pin.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u64>().is_ok()
        })
}

fn valid_tag(tag: &str) -> bool {
    velnor_actions_contract::is_valid_git_ref_fragment(tag)
        && tag.len() <= 256
        && !tag.starts_with('-')
        && !tag.contains(['\'', '"', '{', '}'])
}

fn invalid(problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(problem.to_owned())
}

#[cfg(test)]
mod tests {
    use super::valid_tag;

    #[test]
    fn tag_grammar_preserves_release_specific_boundaries() {
        for tag in [
            "HEAD",
            "v1.2.3+build",
            "a./b",
            "a/-b",
            "a.LOCK",
            "a$b",
            "a;b",
        ] {
            assert!(valid_tag(tag), "{tag:?}");
        }
        assert!(valid_tag(&"a".repeat(256)));
        for tag in [
            "-a", "a'", "a\"", "a{b", "a}b", "a.lock", "a.lock/b", "a/b.lock",
        ] {
            assert!(!valid_tag(tag), "{tag:?}");
        }
        assert!(!valid_tag(&"a".repeat(257)));
    }
}

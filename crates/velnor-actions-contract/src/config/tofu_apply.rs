//! Closed declaration for a protected post-merge `OpenTofu` apply workflow.

use super::tofu::Utf8RepoRelDir;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One protected main-branch apply configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TofuApplyConfig {
    /// One configured validation root to apply.
    pub root: Utf8RepoRelDir,
    /// Existing GitHub environment whose rules protect deployment.
    pub environment: String,
    /// AWS role assumed with GitHub OIDC; no long-lived AWS keys are accepted.
    pub role_arn: String,
    /// Exact S3 state location. Lockfile use is always enabled by Velnor.
    pub backend: S3BackendConfig,
    /// Organization-keyed GitHub provider token secret names.
    pub github_tokens: Vec<GitHubTokenSecret>,
}

/// S3 backend location. Authentication is exclusively job-scoped OIDC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3BackendConfig {
    /// S3 bucket with versioning enabled by the operator.
    pub bucket: String,
    /// Repository-relative object key, separate from other states.
    pub key: String,
    /// AWS region for both the state bucket and assumed role.
    pub region: String,
}

/// Explicit mapping from one GitHub organization to one Actions secret name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubTokenSecret {
    /// Lowercase GitHub organization slug.
    pub organization: String,
    /// Name of the repository/environment secret holding its token.
    pub secret_name: String,
}

impl TofuApplyConfig {
    /// Validate every workflow input before the renderer uses it.
    ///
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if let Err(problem) = Utf8RepoRelDir::parse(self.root.as_str()) {
            return Err(ContractError::config(
                file,
                "workflow.tofu_apply.root",
                format!("invalid_root:{problem:?}"),
            ));
        }
        if !is_valid_environment(&self.environment) {
            return Err(ContractError::config(
                file,
                "workflow.tofu_apply.environment",
                "invalid_environment_name",
            ));
        }
        if !is_valid_role_arn(&self.role_arn) {
            return Err(ContractError::config(
                file,
                "workflow.tofu_apply.role_arn",
                "invalid_aws_role_arn",
            ));
        }
        self.backend.validate(file)?;
        if self.github_tokens.is_empty() {
            return Err(ContractError::config(
                file,
                "workflow.tofu_apply.github_tokens",
                "empty_github_tokens",
            ));
        }
        let mut previous = None;
        let mut organizations = BTreeSet::new();
        let mut secrets = BTreeSet::new();
        for credential in &self.github_tokens {
            if !is_valid_organization(&credential.organization) {
                return Err(ContractError::config(
                    file,
                    "workflow.tofu_apply.github_tokens.organization",
                    "invalid_organization_slug",
                ));
            }
            if !is_valid_secret_name(&credential.secret_name) {
                return Err(ContractError::config(
                    file,
                    "workflow.tofu_apply.github_tokens.secret_name",
                    "invalid_secret_name",
                ));
            }
            if previous.is_some_and(|slug: &str| slug >= credential.organization.as_str()) {
                return Err(ContractError::config(
                    file,
                    "workflow.tofu_apply.github_tokens",
                    "must_be_sorted_unique_by_organization",
                ));
            }
            if !organizations.insert(credential.organization.as_str())
                || !secrets.insert(credential.secret_name.as_str())
            {
                return Err(ContractError::config(
                    file,
                    "workflow.tofu_apply.github_tokens",
                    "duplicate_organization_or_secret",
                ));
            }
            previous = Some(credential.organization.as_str());
        }
        Ok(())
    }
}

impl S3BackendConfig {
    /// Validate S3 names and region as fixed command arguments.
    ///
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !is_valid_bucket(&self.bucket) {
            return Err(ContractError::config(
                file,
                "workflow.tofu_apply.backend.bucket",
                "invalid_s3_bucket",
            ));
        }
        if !is_valid_state_key(&self.key) {
            return Err(ContractError::config(
                file,
                "workflow.tofu_apply.backend.key",
                "invalid_s3_state_key",
            ));
        }
        if !is_valid_region(&self.region) {
            return Err(ContractError::config(
                file,
                "workflow.tofu_apply.backend.region",
                "invalid_aws_region",
            ));
        }
        Ok(())
    }
}

/// GitHub environment names are intentionally narrower than the upstream grammar.
fn is_valid_environment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// AWS role ARNs accepted by the GitHub OIDC credential action.
fn is_valid_role_arn(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("arn:aws:iam::") else {
        return false;
    };
    let Some((account, role)) = rest.split_once(":role/") else {
        return false;
    };
    account.len() == 12
        && account.bytes().all(|byte| byte.is_ascii_digit())
        && !role.is_empty()
        && role.len() <= 512
        && !role.starts_with('/')
        && !role.ends_with('/')
        && !role
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && role.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'+' | b'=' | b',' | b'.' | b'@' | b'_' | b'-' | b'/')
        })
}

/// GitHub organization slug.
fn is_valid_organization(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0].is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Exact AWS S3 bucket-name subset accepted for a state bucket.
fn is_valid_bucket(value: &str) -> bool {
    (3..=63).contains(&value.len())
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0].is_ascii_digit())
        && (value.as_bytes()[value.len() - 1].is_ascii_lowercase()
            || value.as_bytes()[value.len() - 1].is_ascii_digit())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        })
        && !value.contains("..")
        && !value.contains(".-")
        && !value.contains("-.")
}

/// S3 state key with a simple non-escaping path grammar.
fn is_valid_state_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.'))
}

/// Lowercase AWS region identifier.
fn is_valid_region(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// GitHub secret names injected as expressions, never token values.
fn is_valid_secret_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.starts_with("GH_TOKEN_")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && value != "GITHUB_TOKEN"
}

#[cfg(test)]
mod tests {
    use super::{GitHubTokenSecret, S3BackendConfig, TofuApplyConfig};
    use crate::config::Utf8RepoRelDir;

    fn config() -> TofuApplyConfig {
        TofuApplyConfig {
            root: Utf8RepoRelDir::from_raw("infra".to_owned()),
            environment: "production".to_owned(),
            role_arn: "arn:aws:iam::123456789012:role/velnor-tofu".to_owned(),
            backend: S3BackendConfig {
                bucket: "example-tofu-state".to_owned(),
                key: "chainargos/control-plane.tfstate".to_owned(),
                region: "us-east-1".to_owned(),
            },
            github_tokens: vec![
                GitHubTokenSecret {
                    organization: "chainargos".to_owned(),
                    secret_name: "GH_TOKEN_CHAINARGOS".to_owned(),
                },
                GitHubTokenSecret {
                    organization: "tailrocks".to_owned(),
                    secret_name: "GH_TOKEN_TAILROCKS".to_owned(),
                },
            ],
        }
    }

    #[test]
    fn validates_closed_oidc_s3_and_per_organization_inputs() {
        assert!(config().validate("config.toml").is_ok());
        let mut invalid = config();
        invalid.role_arn = "arn:aws:iam::123456789012:role/../admin".to_owned();
        assert!(invalid.validate("config.toml").is_err());
        let mut invalid = config();
        invalid.backend.key = "../shared.tfstate".to_owned();
        assert!(invalid.validate("config.toml").is_err());
        let mut invalid = config();
        invalid.github_tokens[0].secret_name = "${{ secrets.X }}".to_owned();
        assert!(invalid.validate("config.toml").is_err());
        let mut invalid = config();
        invalid.github_tokens.swap(0, 1);
        assert!(invalid.validate("config.toml").is_err());
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::tofu::Utf8RepoRelDir;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TofuApplyConfig {
    pub root: Utf8RepoRelDir,

    pub environment: String,

    pub role_arn: String,

    pub backend: S3BackendConfig,

    pub github_tokens: Vec<GitHubTokenSecret>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct S3BackendConfig {
    pub bucket: String,

    pub key: String,

    pub region: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubTokenSecret {
    pub organization: String,

    pub secret_name: String,
}

impl TofuApplyConfig {
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

fn is_valid_environment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

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

fn is_valid_organization(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 39
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0].is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

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

fn is_valid_region(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_valid_secret_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.starts_with("GH_TOKEN_")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && value != "GITHUB_TOKEN"
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::ActionlintError;

pub const ALLOWED_ACTIONS: [&str; 13] = [
    "jdx/mise-action",
    "actions/checkout",
    "actions/download-artifact",
    "actions/upload-artifact",
    "actions/cache/restore",
    "actions/cache/save",
    "jdx/mr-boxington-action",
    "asamarts/alint",
    "aws-actions/configure-aws-credentials",
    "actions/attest-build-provenance",
    "actions/attest",
    "actions/create-github-app-token",
    "actions/github-script",
];

pub const GITHUB_SCRIPT_ACTION: &str = "actions/github-script";

pub const GITHUB_SCRIPT_ACTION_VERSION: &str = "v9.0.0";

pub const GITHUB_SCRIPT_ACTION_SHA: &str = "3a2844b7e9c422d3c10d287c895573f7108da1b3";

pub const GITHUB_SCRIPT_ACTION_USES: &str =
    "actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3";

pub const CREATE_GITHUB_APP_TOKEN_ACTION: &str = "actions/create-github-app-token";

pub const CREATE_GITHUB_APP_TOKEN_ACTION_VERSION: &str = "v3.3.0";

pub const CREATE_GITHUB_APP_TOKEN_ACTION_SHA: &str = "64ab729a6294000f5aa4a4bc0b73d1f32bc45510";

pub const CREATE_GITHUB_APP_TOKEN_USES: &str =
    "actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510";

pub const ATTEST_BUILD_PROVENANCE_ACTION: &str = "actions/attest-build-provenance";

pub const ATTEST_BUILD_PROVENANCE_ACTION_VERSION: &str = "v4.2.2";

pub const ATTEST_BUILD_PROVENANCE_ACTION_SHA: &str = "4d101475d8b20a2381f78447822ac1eab6504dd8";

pub const ATTEST_BUILD_PROVENANCE_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";

pub const ATTEST_ACTION: &str = "actions/attest";

pub const ATTEST_ACTION_VERSION: &str = "v4.2.2";

pub const ATTEST_ACTION_SHA: &str = "1e69f48acb82d1966a394da916b4c1698aa569d6";

pub const ATTEST_USES: &str = "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6";

pub const ALINT_ACTION: &str = "asamarts/alint";

pub const CHECKOUT_ACTION: &str = "actions/checkout";

pub const CHECKOUT_ACTION_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

pub const AWS_CREDENTIALS_ACTION: &str = "aws-actions/configure-aws-credentials";

pub const AWS_CREDENTIALS_ACTION_VERSION: &str = "v6.3.0";

pub const AWS_CREDENTIALS_ACTION_SHA: &str = "e1253824e5c10ff9df46874f81ed3ec929e19cfd";

pub const ALINT_ACTION_VERSION: &str = "v0.20.1";

pub const ALINT_ACTION_SHA: &str = "265d9fdc720bb8f59d00b87e33e4149869f77327";

pub const MISE_ACTION_VERSION: &str = "v5.2.4";

pub const MISE_ACTION_SHA: &str = "6be3c7ed4c21d4f76b208d351d95aac273ebb76d";

pub const CHECKOUT_ACTION_VERSION: &str = "v7.0.1";

pub const CHECKOUT_ACTION_SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";

pub const DOWNLOAD_ARTIFACT_ACTION_VERSION: &str = "v8.0.2";

pub const DOWNLOAD_ARTIFACT_ACTION_SHA: &str = "9000827ccba6bdab643e8b6fd33ac0654aef8333";

pub const DOWNLOAD_ARTIFACT_ACTION_USES: &str =
    "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333";

pub const UPLOAD_ARTIFACT_ACTION_VERSION: &str = "v7.0.2";

pub const UPLOAD_ARTIFACT_ACTION_SHA: &str = "cf430e030ddbb5b0abf93d22962f4752f3646cd9";

pub const UPLOAD_ARTIFACT_ACTION_USES: &str =
    "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9";

pub const CACHE_ACTION_VERSION: &str = "v6.1.0";

pub const CACHE_ACTION_SHA: &str = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

pub const MR_BOXINGTON_ACTION_VERSION: &str = "v1.7.3";

pub const MR_BOXINGTON_ACTION_SHA: &str = "88acf2bb50552c7c508cbac13e5f8c576b95b1de";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedActionRef {
    pub repo: String,

    pub path: Option<String>,

    pub sha: String,

    pub version_comment: String,
}

impl PinnedActionRef {
    #[must_use]
    pub fn aws_credentials() -> Self {
        Self {
            repo: AWS_CREDENTIALS_ACTION.to_owned(),
            path: None,
            sha: AWS_CREDENTIALS_ACTION_SHA.to_owned(),
            version_comment: AWS_CREDENTIALS_ACTION_VERSION.to_owned(),
        }
    }

    pub fn new(
        repo: &str,
        path: Option<&str>,
        sha: &str,
        version_comment: &str,
    ) -> Result<Self, ActionlintError> {
        let candidate = Self {
            repo: repo.to_owned(),
            path: path.map(str::to_owned),
            sha: sha.to_owned(),
            version_comment: version_comment.to_owned(),
        };
        candidate.validate()?;
        Ok(candidate)
    }

    #[must_use]
    pub fn alint() -> Self {
        Self {
            repo: ALINT_ACTION.to_owned(),
            path: None,
            sha: ALINT_ACTION_SHA.to_owned(),
            version_comment: ALINT_ACTION_VERSION.to_owned(),
        }
    }

    #[must_use]
    pub fn checkout() -> Self {
        Self {
            repo: CHECKOUT_ACTION.to_owned(),
            path: None,
            sha: CHECKOUT_ACTION_SHA.to_owned(),
            version_comment: CHECKOUT_ACTION_VERSION.to_owned(),
        }
    }

    #[must_use]
    pub fn uses_key(&self) -> String {
        match &self.path {
            Some(path) => format!("{}/{path}", self.repo),
            None => self.repo.clone(),
        }
    }

    #[must_use]
    pub fn uses_value(&self) -> String {
        format!("{}@{}", self.uses_key(), self.sha)
    }

    #[must_use]
    pub fn render_uses(&self) -> String {
        format!("uses: {} # {}", self.uses_value(), self.version_comment)
    }

    pub fn parse_uses(value: &str, version_comment: &str) -> Result<Self, ActionlintError> {
        let (key, reference) =
            value
                .split_once('@')
                .ok_or_else(|| ActionlintError::InvalidPin {
                    uses: value.to_owned(),
                    problem: "missing_at_ref".to_owned(),
                })?;
        if !is_valid_repo_key(key) {
            return Err(ActionlintError::InvalidPin {
                uses: value.to_owned(),
                problem: format!("invalid_key:{key}"),
            });
        }
        if !ALLOWED_ACTIONS.contains(&key) {
            return Err(ActionlintError::UnknownAction {
                uses: value.to_owned(),
            });
        }
        if !is_version_tag(version_comment) {
            return Err(ActionlintError::InvalidPin {
                uses: value.to_owned(),
                problem: format!("invalid_version_comment:{version_comment}"),
            });
        }
        if !is_full_sha(reference) {
            return Err(ActionlintError::InvalidPin {
                uses: value.to_owned(),
                problem: "ref_must_be_full_sha".to_owned(),
            });
        }
        let (repo, path) = split_key(key);
        Self::new(&repo, path.as_deref(), reference, version_comment)
    }

    pub fn validate(&self) -> Result<(), ActionlintError> {
        let key = self.uses_key();
        let uses = format!("{key}@{}", self.sha);
        if !ALLOWED_ACTIONS.contains(&key.as_str()) {
            return Err(ActionlintError::UnknownAction { uses });
        }
        if !is_version_tag(&self.version_comment) {
            return Err(ActionlintError::InvalidPin {
                uses,
                problem: format!("invalid_version_comment:{}", self.version_comment),
            });
        }
        if !is_full_sha(&self.sha) {
            return Err(ActionlintError::InvalidPin {
                uses,
                problem: "ref_must_be_full_sha".to_owned(),
            });
        }
        Ok(())
    }
}

pub(crate) fn split_key(key: &str) -> (String, Option<String>) {
    let mut parts = key.splitn(3, '/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    let path = parts.next().map(str::to_owned);
    (format!("{owner}/{repo}"), path)
}

fn is_valid_repo_key(key: &str) -> bool {
    if key.is_empty() || key.contains('@') || key.contains('#') {
        return false;
    }
    let mut parts = key.split('/');
    let (Some(owner), Some(repo)) = (parts.next(), parts.next()) else {
        return false;
    };
    if !is_key_segment(owner) || !is_key_segment(repo) {
        return false;
    }
    parts.all(is_key_segment)
}

fn is_key_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub(crate) fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

pub(crate) fn is_version_tag(value: &str) -> bool {
    let Some(number) = value.strip_prefix('v') else {
        return false;
    };
    let parts: Vec<&str> = number.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

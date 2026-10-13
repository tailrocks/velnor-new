//! Pinned action refs against the 8-entry allowlist.
//!
//! Ordinary refs pin `repo[/path]@sha` plus a `# vX.Y.Z` comment; the
//! Alint tag is the sole reviewed mutable-tag exception.

use crate::ActionlintError;

/// Exhaustive allowlist of `owner/repo[/path]` action keys.
pub const ALLOWED_ACTIONS: [&str; 8] = [
    "jdx/mise-action",
    "actions/checkout",
    "actions/download-artifact",
    "actions/upload-artifact",
    "actions/cache/restore",
    "actions/cache/save",
    "jdx/mr-boxington-action",
    "asamarts/alint",
];

/// Action key holding the sole mutable-tag exception.
pub const ALINT_ACTION: &str = "asamarts/alint";

/// Reviewed Alint tag; changing it is a version-policy update, not config.
pub const ALINT_REVIEWED_TAG: &str = "v0.16.1";

/// One pinned action reference: `repo[/path]@sha` plus version comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedActionRef {
    /// Action repository (`owner/repo`).
    pub repo: String,
    /// Sub-action path (e.g. `restore` for `actions/cache/restore`).
    pub path: Option<String>,
    /// Full 40-char SHA; `None` only for the Alint tag exception.
    pub sha: Option<String>,
    /// Matching `# vX.Y.Z` comment text.
    pub version_comment: String,
    /// True only for the reviewed Alint mutable tag.
    pub tag_exception: bool,
}

impl PinnedActionRef {
    /// Build and validate an ordinary SHA-pinned ref.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError`] when the key is not allowlisted, the
    /// SHA is not 40 lowercase hex, or the version comment is not `vX.Y.Z`.
    pub fn new(
        repo: &str,
        path: Option<&str>,
        sha: &str,
        version_comment: &str,
    ) -> Result<Self, ActionlintError> {
        let candidate = Self {
            repo: repo.to_owned(),
            path: path.map(str::to_owned),
            sha: Some(sha.to_owned()),
            version_comment: version_comment.to_owned(),
            tag_exception: false,
        };
        candidate.validate()?;
        Ok(candidate)
    }

    /// Build the reviewed Alint mutable-tag ref.
    #[must_use]
    pub fn alint() -> Self {
        Self {
            repo: "asamarts".to_owned(),
            path: Some("alint".to_owned()),
            sha: None,
            version_comment: ALINT_REVIEWED_TAG.to_owned(),
            tag_exception: true,
        }
    }

    /// Full action key: `repo` plus optional `/path`.
    #[must_use]
    pub fn uses_key(&self) -> String {
        match &self.path {
            Some(path) => format!("{}/{path}", self.repo),
            None => self.repo.clone(),
        }
    }

    /// Render the deterministic `uses:` line with version comment.
    #[must_use]
    pub fn render_uses(&self) -> String {
        let reference = self
            .sha
            .clone()
            .unwrap_or_else(|| ALINT_REVIEWED_TAG.to_owned());
        format!(
            "uses: {}@{reference} # {}",
            self.uses_key(),
            self.version_comment
        )
    }

    /// Parse `repo[/path]@ref` with an expected version comment.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError`] for unknown actions, branch or moving
    /// refs, installer actions, and malformed pins or comments.
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
        if key == ALINT_ACTION {
            return Self::parse_alint(value, reference, version_comment);
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

    /// Validate allowlist membership, pin shape, and version comment.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError`] for unknown actions, malformed SHAs,
    /// misplaced tag exceptions, and bad version comments.
    pub fn validate(&self) -> Result<(), ActionlintError> {
        let key = self.uses_key();
        let uses = format!(
            "{key}@{}",
            self.sha.as_deref().unwrap_or(ALINT_REVIEWED_TAG)
        );
        if !ALLOWED_ACTIONS.contains(&key.as_str()) {
            return Err(ActionlintError::UnknownAction { uses });
        }
        if !is_version_tag(&self.version_comment) {
            return Err(ActionlintError::InvalidPin {
                uses,
                problem: format!("invalid_version_comment:{}", self.version_comment),
            });
        }
        if self.tag_exception {
            return self.validate_tag_exception(&uses);
        }
        match &self.sha {
            Some(sha) if is_full_sha(sha) => Ok(()),
            _ => Err(ActionlintError::InvalidPin {
                uses,
                problem: "ref_must_be_full_sha".to_owned(),
            }),
        }
    }

    /// Validate the Alint-only tag exception fields.
    fn validate_tag_exception(&self, uses: &str) -> Result<(), ActionlintError> {
        if self.uses_key() != ALINT_ACTION || self.sha.is_some() {
            return Err(ActionlintError::InvalidPin {
                uses: uses.to_owned(),
                problem: "tag_exception_alint_only".to_owned(),
            });
        }
        if self.version_comment != ALINT_REVIEWED_TAG {
            return Err(ActionlintError::InvalidPin {
                uses: uses.to_owned(),
                problem: format!("alint_tag_must_be:{ALINT_REVIEWED_TAG}"),
            });
        }
        Ok(())
    }

    /// Parse the Alint `uses:` value against the reviewed tag.
    fn parse_alint(
        value: &str,
        reference: &str,
        version_comment: &str,
    ) -> Result<Self, ActionlintError> {
        if reference != ALINT_REVIEWED_TAG || version_comment != ALINT_REVIEWED_TAG {
            return Err(ActionlintError::InvalidPin {
                uses: value.to_owned(),
                problem: format!("alint_tag_must_be:{ALINT_REVIEWED_TAG}"),
            });
        }
        Ok(Self::alint())
    }
}

/// Split an action key into repo plus optional sub-action path.
pub(crate) fn split_key(key: &str) -> (String, Option<String>) {
    let mut parts = key.splitn(3, '/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    let path = parts.next().map(str::to_owned);
    (format!("{owner}/{repo}"), path)
}

/// Repo keys are `owner/repo[/path]` without refs or whitespace.
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

/// One `/`-separated key segment.
fn is_key_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Full commit SHA: 40 lowercase hex characters.
pub(crate) fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Stable version tags: `vX.Y.Z` with numeric parts.
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

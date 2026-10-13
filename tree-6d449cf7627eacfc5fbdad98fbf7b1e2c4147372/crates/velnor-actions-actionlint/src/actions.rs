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

/// Action key for the no-credentials checkout every job embeds.
pub const CHECKOUT_ACTION: &str = "actions/checkout";

/// Reviewed Alint tag; changing it is a version-policy update, not config.
/// Sole spec-blessed mutable-tag exception (`docs/proposed/version-policy.md` §2).
/// Source: `https://api.github.com/repos/asamarts/alint/releases/latest`; checked 2026-09-28.
pub const ALINT_REVIEWED_TAG: &str = "v0.16.1";

/// Qualified `jdx/mise-action` release.
/// Source: `https://api.github.com/repos/jdx/mise-action/releases/latest`; checked 2026-09-28.
pub const MISE_ACTION_VERSION: &str = "v4.3.0";
/// Full commit SHA for [`MISE_ACTION_VERSION`] (tag object type `commit`).
pub const MISE_ACTION_SHA: &str = "c2a87611a18de5b3828c5652fe268e992400cb5c";
/// Qualified `actions/checkout` release.
/// Source: `https://api.github.com/repos/actions/checkout/releases/latest`; checked 2026-09-28.
pub const CHECKOUT_ACTION_VERSION: &str = "v7.0.1";
/// Full commit SHA for [`CHECKOUT_ACTION_VERSION`] (`v7` moves with it).
pub const CHECKOUT_ACTION_SHA: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";
/// Qualified `actions/download-artifact` release.
/// Source: `https://api.github.com/repos/actions/download-artifact/releases/latest`; checked 2026-09-28.
pub const DOWNLOAD_ARTIFACT_ACTION_VERSION: &str = "v8.0.1";
/// Full commit SHA for [`DOWNLOAD_ARTIFACT_ACTION_VERSION`] (`v8` moves with it).
pub const DOWNLOAD_ARTIFACT_ACTION_SHA: &str = "3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c";
/// Qualified `actions/upload-artifact` release.
/// Source: `https://api.github.com/repos/actions/upload-artifact/releases/latest`; checked 2026-09-28.
pub const UPLOAD_ARTIFACT_ACTION_VERSION: &str = "v7.0.1";
/// Full commit SHA for [`UPLOAD_ARTIFACT_ACTION_VERSION`] (`v7` moves with it).
pub const UPLOAD_ARTIFACT_ACTION_SHA: &str = "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a";
/// Qualified `actions/cache` release (shared by `restore` and `save`).
/// Source: `https://api.github.com/repos/actions/cache/releases/latest`; checked 2026-09-28.
pub const CACHE_ACTION_VERSION: &str = "v6.1.0";
/// Full commit SHA for [`CACHE_ACTION_VERSION`] (`v6` moves with it; v6 is current, no replacement).
pub const CACHE_ACTION_SHA: &str = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Qualified `jdx/mr-boxington-action` release.
/// Source: `https://api.github.com/repos/jdx/mr-boxington-action/releases`; checked 2026-09-28.
pub const MR_BOXINGTON_ACTION_VERSION: &str = "v1.5.0";
/// Full commit SHA for [`MR_BOXINGTON_ACTION_VERSION`] (`v1` moves with it).
pub const MR_BOXINGTON_ACTION_SHA: &str = "9df1d4b18b2147788a7ee7a2c7b84ecf62fd89d3";

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

    /// Canonical qualified checkout ref for job `uses:` values.
    ///
    /// Binds [`CHECKOUT_ACTION_SHA`] to [`CHECKOUT_ACTION_VERSION`] so
    /// emitters never hand-write the pin. Valid by construction.
    #[must_use]
    pub fn checkout() -> Self {
        Self {
            repo: CHECKOUT_ACTION.to_owned(),
            path: None,
            sha: Some(CHECKOUT_ACTION_SHA.to_owned()),
            version_comment: CHECKOUT_ACTION_VERSION.to_owned(),
            tag_exception: false,
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

    /// Bare `key@ref` value carried by `StepKind::Action` payloads.
    ///
    /// Comment-free: the renderer emits this verbatim as the `uses:`
    /// scalar, so emitters must pass it through unmodified.
    #[must_use]
    pub fn uses_value(&self) -> String {
        let reference = self
            .sha
            .clone()
            .unwrap_or_else(|| ALINT_REVIEWED_TAG.to_owned());
        format!("{}@{reference}", self.uses_key())
    }

    /// Render the deterministic `uses:` line with version comment.
    #[must_use]
    pub fn render_uses(&self) -> String {
        format!("uses: {} # {}", self.uses_value(), self.version_comment)
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

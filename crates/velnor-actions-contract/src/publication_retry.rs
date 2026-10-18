//! Non-overwriting publication transitions over independently authenticated observations.
//!
//! This planner performs no authentication or I/O. Callers must verify the
//! canonical receipt, source/authority/run identity, retained evidence, and
//! observed tag/assets before constructing these inputs. Every mutation must
//! recheck eligibility; a plan is not a reusable authorization token.

use std::collections::{BTreeMap, BTreeSet};

/// Authenticated immutable receipt identity, including its complete asset set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationIdentity {
    /// Exact source commit expected at the owned version tag.
    pub source_sha: String,
    /// Canonical receipt digest binds channel, version, authority, run, and bytes.
    pub receipt_digest: String,
    /// Complete release assets, including receipt and its detached bundle.
    pub assets: BTreeMap<String, String>,
}

/// Independently observed remote state at the deterministic owned tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicationState {
    /// Neither tag nor release exists.
    Absent,
    /// A tag exists without a draft or published release.
    TagOnly {
        /// Full commit to which the observed tag resolves.
        source_sha: String,
        /// Receipt digest embedded in the authenticated annotated tag message.
        original_receipt_digest: Option<String>,
    },
    /// Unpublished draft; unknown original authority is represented by `None`.
    Draft {
        /// Full commit to which the draft tag resolves.
        source_sha: String,
        /// Retained authenticated receipt identity from the original build run.
        original_receipt_digest: Option<String>,
        /// Existing remote assets and their independently verified digests.
        assets: BTreeMap<String, String>,
    },
    /// A visible release whose immutable status and exact assets were inspected.
    Published {
        /// Authenticated original receipt and complete observed asset inventory.
        identity: PublicationIdentity,
        /// Immutable status read from the authoritative release API.
        immutable: bool,
    },
}

/// Sole permitted next transition; no branch authorizes overwriting any object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicationAction {
    /// Create a tag without force, then a draft.
    CreateTagAndDraft,
    /// Resume after tag creation, preserving the exact existing tag.
    CreateDraft,
    /// Upload only these missing original assets, then publish after rechecks.
    CompleteDraft {
        /// Declared original assets absent from the observed remote draft.
        missing_assets: Vec<String>,
    },
    /// Publication is complete; only downstream reconciliation may resume.
    ReconcileDistribution,
    /// Stop for inspected recovery; never move tags or replace assets.
    Reject(PublicationRejection),
}

/// Closed fail-closed publication conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationRejection {
    /// An unfinished candidate is no longer the authorized default-branch head.
    StaleCandidate,
    /// Existing tag resolves to a different commit.
    ConflictingTag,
    /// Existing draft has no authenticated original receipt/run authority.
    UnknownPublicationOrigin,
    /// Original receipt or existing asset identity conflicts with expected bytes.
    ConflictingIdentity,
    /// Required missing asset bytes/evidence from the original run are unavailable.
    UnavailableOriginalAssets,
    /// Published release is not immutable.
    MutablePublishedRelease,
}

/// Select a non-overwriting transition using verified receipt and remote state.
///
/// `available_original_assets` names assets whose exact original bytes and
/// evidence remain authenticated and available. It must never describe a new
/// build merely sharing source/version. Completed releases may reconcile after
/// source advances; downstream freshness/downgrade checks still apply.
#[must_use]
pub fn publication_action(
    expected: &PublicationIdentity,
    observed: &PublicationState,
    candidate_current: bool,
    available_original_assets: &BTreeSet<String>,
) -> PublicationAction {
    use PublicationAction::{ReconcileDistribution, Reject};
    use PublicationRejection as Rejection;
    if let PublicationState::Published {
        identity,
        immutable,
    } = observed
    {
        return if !immutable {
            Reject(Rejection::MutablePublishedRelease)
        } else if identity != expected {
            Reject(Rejection::ConflictingIdentity)
        } else {
            ReconcileDistribution
        };
    }
    if !candidate_current {
        return Reject(Rejection::StaleCandidate);
    }
    match observed {
        PublicationState::Absent => {
            if expected
                .assets
                .keys()
                .all(|name| available_original_assets.contains(name))
            {
                PublicationAction::CreateTagAndDraft
            } else {
                Reject(Rejection::UnavailableOriginalAssets)
            }
        }
        PublicationState::TagOnly {
            source_sha,
            original_receipt_digest,
        } => {
            if source_sha != &expected.source_sha {
                Reject(Rejection::ConflictingTag)
            } else if original_receipt_digest.as_deref() != Some(expected.receipt_digest.as_str()) {
                if original_receipt_digest.is_none() {
                    Reject(Rejection::UnknownPublicationOrigin)
                } else {
                    Reject(Rejection::ConflictingIdentity)
                }
            } else if expected
                .assets
                .keys()
                .all(|name| available_original_assets.contains(name))
            {
                PublicationAction::CreateDraft
            } else {
                Reject(Rejection::UnavailableOriginalAssets)
            }
        }
        PublicationState::Draft {
            source_sha,
            original_receipt_digest,
            assets,
        } => draft_action(
            expected,
            source_sha,
            original_receipt_digest.as_deref(),
            assets,
            available_original_assets,
        ),
        PublicationState::Published { .. } => ReconcileDistribution,
    }
}

fn draft_action(
    expected: &PublicationIdentity,
    source_sha: &str,
    original_receipt_digest: Option<&str>,
    assets: &BTreeMap<String, String>,
    available: &BTreeSet<String>,
) -> PublicationAction {
    use PublicationAction::Reject;
    use PublicationRejection as Rejection;
    if source_sha != expected.source_sha {
        return Reject(Rejection::ConflictingTag);
    }
    let Some(original) = original_receipt_digest else {
        return Reject(Rejection::UnknownPublicationOrigin);
    };
    if original != expected.receipt_digest
        || assets
            .iter()
            .any(|(name, digest)| expected.assets.get(name) != Some(digest))
    {
        return Reject(Rejection::ConflictingIdentity);
    }
    let missing_assets: Vec<String> = ordered_asset_names(expected)
        .into_iter()
        .filter(|name| !assets.contains_key(name))
        .collect();
    if missing_assets.iter().any(|name| !available.contains(name)) {
        return Reject(Rejection::UnavailableOriginalAssets);
    }
    PublicationAction::CompleteDraft { missing_assets }
}

/// Upload the signed receipt first so a partial draft can prove its origin.
fn ordered_asset_names(expected: &PublicationIdentity) -> Vec<String> {
    let mut names = expected.assets.keys().cloned().collect::<Vec<_>>();
    names.sort_by(|left, right| {
        asset_upload_rank(left)
            .cmp(&asset_upload_rank(right))
            .then_with(|| left.cmp(right))
    });
    names
}

fn asset_upload_rank(name: &str) -> u8 {
    match name {
        "release.json" => 0,
        "release.json.bundle" => 1,
        _ => 2,
    }
}

#[cfg(test)]
#[path = "publication_retry_tests.rs"]
mod tests;

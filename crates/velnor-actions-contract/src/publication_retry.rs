// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationIdentity {
    pub source_sha: String,

    pub receipt_digest: String,

    pub assets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicationState {
    Absent,

    TagOnly {
        source_sha: String,

        original_receipt_digest: Option<String>,
    },

    AuthorizedStableTag {
        identity: PublicationIdentity,
    },

    Draft {
        source_sha: String,

        original_receipt_digest: Option<String>,

        assets: BTreeMap<String, String>,
    },

    Published {
        identity: PublicationIdentity,

        immutable: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicationAction {
    CreateTag,

    CreateDraft,

    CompleteDraft { missing_assets: Vec<String> },

    ReconcileDistribution,

    Reject(PublicationRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationRejection {
    StaleCandidate,

    ConflictingTag,

    UnknownPublicationOrigin,

    ConflictingIdentity,

    UnavailableOriginalAssets,

    MutablePublishedRelease,
}

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
                PublicationAction::CreateTag
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
        PublicationState::AuthorizedStableTag { identity } => {
            if identity != expected {
                Reject(Rejection::ConflictingIdentity)
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

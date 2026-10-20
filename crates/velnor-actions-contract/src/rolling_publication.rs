//! Rolling-channel transitions inside the native package publication system.
//!
//! The backend authenticates the original signed generation and latest-main CI,
//! fixes the existing mutable release ID, and verifies tag ancestry. GitHub has
//! no atomic asset-set replacement: staging and replacement remain publicly
//! observable. Only a committed, exact inventory may authorize distribution.

use std::collections::{BTreeMap, BTreeSet};

use super::PublicationIdentity;

#[cfg(test)]
#[path = "rolling_publication_tests.rs"]
mod tests;
#[path = "rolling_publication_validation.rs"]
mod validation;
use validation::{lower_hex, valid_observation};

#[path = "rolling_publication_types.rs"]
mod types;
pub use types::{
    RollingAsset, RollingAssetState, RollingJournal, RollingOriginal, RollingPhase,
    RollingReleaseStatus, RollingState,
};

/// One bounded mutation or a terminal decision; every mutation needs fresh CI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollingAction {
    /// Upload original signed bytes under their generation-specific stage name.
    UploadStage(String),
    /// Remove only an unfinished upload belonging to this exact generation.
    DeleteStarter(u64),
    /// Persist the original and predecessor intent before uploading anything.
    Begin(RollingJournal),
    /// Freeze the complete staged IDs before deleting any predecessor.
    Ready(RollingJournal),
    /// Delete one still-matching predecessor asset by database ID.
    DeleteObsolete(u64),
    /// Rename one authenticated staged object to its canonical filename.
    Rename {
        /// Exact authenticated staged object ID.
        id: u64,
        /// Canonical name from the signed inventory.
        name: String,
    },
    /// Advance the existing preview reference without force or deletion.
    AdvanceTag,
    /// Commit the generation marker after tag and exact inventory admission.
    Commit,
    /// This exact current generation is already committed and may reconcile.
    Reconcile,
    /// Refuse without changing any remote object.
    Reject(RollingRejection),
}

/// Closed rolling conflicts, distinct from immutable stable retry semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollingRejection {
    /// The observed object is not the configured preview release.
    WrongRelease,
    /// A release cannot safely be edited in place.
    ImmutableOrDraft,
    /// The candidate no longer has exact latest-main authority.
    StaleCandidate,
    /// Advancing this tag would replace a descendant or unrelated source.
    TagWouldRewind,
    /// One source has incompatible original signed run identities.
    ConflictingRetry,
    /// Asset identities or required receipt fields are malformed or ambiguous.
    InvalidInventory,
    /// An uploaded object's bytes differ from its authenticated original.
    ConflictingAsset,
    /// An observed object changed outside the admitted transaction.
    JournalChanged,
    /// A predecessor object has no authenticated deletion authority.
    UnadmittedPredecessor,
    /// Required original signed bytes are no longer available.
    UnavailableOriginal,
}

/// Name staged bytes by the complete original receipt digest, never run order.
#[must_use]
pub fn staged_name(identity: &PublicationIdentity, name: &str) -> String {
    format!("vlnr-{}--{name}", identity.receipt_digest)
}

/// Select one transition from freshly authenticated original and remote state.
///
/// `current` includes source/workflow/CI/environment authority, not just ancestry.
/// `available` contains only independently verified original signed asset bytes.
/// An old run never receives even cleanup authority after losing currentness.
#[must_use]
pub fn rolling_action(
    expected: &PublicationIdentity,
    release_id: u64,
    observed: &RollingState,
    current: bool,
    available: &BTreeSet<String>,
) -> RollingAction {
    use RollingAction::Reject;
    use RollingRejection as R;
    if release_id == 0 || observed.release_id != release_id || observed.tag != "preview" {
        return Reject(R::WrongRelease);
    }
    if observed.status != RollingReleaseStatus::MutablePreview {
        return Reject(R::ImmutableOrDraft);
    }
    if !current {
        return Reject(R::StaleCandidate);
    }
    if !observed.tag_can_fast_forward {
        return Reject(R::TagWouldRewind);
    }
    if !valid_observation(expected, &observed.assets)
        || observed.original.run_id == 0
        || observed.original.run_attempt == 0
        || observed.original.artifact_id == 0
        || !lower_hex(&observed.original.archive_sha256, 64)
    {
        return Reject(R::InvalidInventory);
    }
    for identity in observed
        .published
        .iter()
        .chain(observed.journal.iter().map(|item| &item.identity))
    {
        if identity.source_sha == expected.source_sha && identity != expected {
            return Reject(R::ConflictingRetry);
        }
    }
    if let Some(journal) = &observed.journal {
        if journal.identity == *expected {
            if journal.original != observed.original {
                return Reject(R::ConflictingRetry);
            }
            if journal.phase == RollingPhase::Staging {
                return stage(expected, observed, journal, available);
            }
            return resume(expected, observed, journal);
        }
        if !observed.journal_can_advance {
            return Reject(R::JournalChanged);
        }
    }
    if observed
        .assets
        .iter()
        .any(|asset| !observed.replaceable.contains(asset))
    {
        return Reject(R::UnadmittedPredecessor);
    }
    RollingAction::Begin(RollingJournal {
        identity: expected.clone(),
        original: observed.original.clone(),
        phase: RollingPhase::Staging,
        staged: BTreeMap::new(),
        obsolete: observed.assets.clone(),
    })
}

fn stage(
    expected: &PublicationIdentity,
    observed: &RollingState,
    journal: &RollingJournal,
    available: &BTreeSet<String>,
) -> RollingAction {
    let mut staged = BTreeMap::new();
    let expected_names: BTreeSet<_> = expected
        .assets
        .keys()
        .map(|name| staged_name(expected, name))
        .collect();
    if !journal.staged.is_empty()
        || journal
            .obsolete
            .iter()
            .any(|asset| !observed.replaceable.contains(asset) || !observed.assets.contains(asset))
        || observed
            .assets
            .iter()
            .any(|item| !expected_names.contains(&item.name) && !journal.obsolete.contains(item))
    {
        return RollingAction::Reject(RollingRejection::UnadmittedPredecessor);
    }
    for (name, digest) in &expected.assets {
        let stage_name = staged_name(expected, name);
        let asset = observed.assets.iter().find(|item| item.name == stage_name);
        match asset {
            Some(item) if item.state == RollingAssetState::Starter => {
                return RollingAction::DeleteStarter(item.id);
            }
            Some(item)
                if item.state == RollingAssetState::Uploaded
                    && item.sha256.as_ref() == Some(digest) =>
            {
                staged.insert(name.clone(), item.clone());
            }
            Some(_) => return RollingAction::Reject(RollingRejection::ConflictingAsset),
            None if available.contains(name) => return RollingAction::UploadStage(name.clone()),
            None => return RollingAction::Reject(RollingRejection::UnavailableOriginal),
        }
    }
    RollingAction::Ready(RollingJournal {
        identity: expected.clone(),
        original: journal.original.clone(),
        phase: RollingPhase::Replacing,
        staged,
        obsolete: journal.obsolete.clone(),
    })
}

fn resume(
    expected: &PublicationIdentity,
    observed: &RollingState,
    journal: &RollingJournal,
) -> RollingAction {
    let staged_ids: BTreeSet<_> = journal.staged.values().map(|item| item.id).collect();
    let obsolete_ids: BTreeSet<_> = journal.obsolete.iter().map(|item| item.id).collect();
    if journal
        .obsolete
        .iter()
        .any(|asset| !observed.replaceable.contains(asset))
        || journal.staged.keys().ne(expected.assets.keys())
        || staged_ids.len() != journal.staged.len()
        || obsolete_ids.len() != journal.obsolete.len()
        || !staged_ids.is_disjoint(&obsolete_ids)
        || observed
            .assets
            .iter()
            .any(|item| !staged_ids.contains(&item.id) && !obsolete_ids.contains(&item.id))
    {
        return RollingAction::Reject(RollingRejection::JournalChanged);
    }
    for (name, asset) in &journal.staged {
        let Some(actual) = observed.assets.iter().find(|item| item.id == asset.id) else {
            return RollingAction::Reject(RollingRejection::JournalChanged);
        };
        if asset.name != staged_name(expected, name)
            || asset.sha256.as_ref() != expected.assets.get(name)
            || asset.size == 0
            || asset.state != RollingAssetState::Uploaded
            || actual.state != RollingAssetState::Uploaded
            || actual.sha256 != asset.sha256
            || actual.size != asset.size
            || (actual.name != asset.name && actual.name != *name)
        {
            return RollingAction::Reject(RollingRejection::ConflictingAsset);
        }
    }
    for old in &journal.obsolete {
        if let Some(actual) = observed.assets.iter().find(|item| item.id == old.id) {
            return if actual == old && journal.phase != RollingPhase::Committed {
                RollingAction::DeleteObsolete(old.id)
            } else {
                RollingAction::Reject(RollingRejection::JournalChanged)
            };
        }
    }
    finish(expected, observed, journal)
}

fn finish(
    expected: &PublicationIdentity,
    observed: &RollingState,
    journal: &RollingJournal,
) -> RollingAction {
    let mut names: Vec<_> = expected.assets.keys().collect();
    names.sort_by_key(|name| (name.as_str() == "release.json", name.as_str()));
    for name in names {
        let asset = &journal.staged[name];
        let Some(actual) = observed.assets.iter().find(|item| item.id == asset.id) else {
            return RollingAction::Reject(RollingRejection::JournalChanged);
        };
        if actual.name != *name {
            if journal.phase == RollingPhase::Committed {
                return RollingAction::Reject(RollingRejection::JournalChanged);
            }
            if name == "release.json" && observed.tag_source != expected.source_sha {
                return RollingAction::AdvanceTag;
            }
            return if journal.phase == RollingPhase::Committed {
                RollingAction::Reject(RollingRejection::JournalChanged)
            } else {
                RollingAction::Rename {
                    id: asset.id,
                    name: name.clone(),
                }
            };
        }
    }
    if observed.tag_source != expected.source_sha {
        return if journal.phase == RollingPhase::Committed {
            RollingAction::Reject(RollingRejection::JournalChanged)
        } else {
            RollingAction::AdvanceTag
        };
    }
    if journal.phase == RollingPhase::Committed {
        RollingAction::Reconcile
    } else {
        RollingAction::Commit
    }
}

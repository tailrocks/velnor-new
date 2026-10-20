//! Signed-generation closure for exact remote deletion identities.

use super::super::{
    identity::{local_identity, read_regular},
    recovery::rolling_original,
};
use super::{Envelope, GitHubPackagePublisher, Predecessor, internal};
use crate::OrchestratorError;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
};
use velnor_actions_contract::publication_retry::{
    PublicationIdentity,
    rolling::{
        RollingAsset, RollingAssetState, RollingBootstrapSnapshot, RollingOriginal, RollingPhase,
        staged_name,
    },
};

const MAX_HISTORY: usize = 8;

#[derive(Default)]
pub(super) struct EvidenceCache(Vec<Generation>);
#[derive(Clone)]
struct Generation {
    original: RollingOriginal,
    identity: PublicationIdentity,
    sizes: BTreeMap<String, u64>,
    version: super::version::Version,
}

impl EvidenceCache {
    fn verified(
        &mut self,
        publisher: &GitHubPackagePublisher,
        original: &RollingOriginal,
    ) -> Result<Generation, OrchestratorError> {
        if let Some(generation) = self.0.iter().find(|item| item.original == *original) {
            return Ok(generation.clone());
        }
        if self.0.len() >= MAX_HISTORY {
            return Err(internal("rolling_original_cache_bound"));
        }
        let current = super::super::published::original_pointer(&publisher.api, &publisher.signed)?;
        let generation = select_original(
            original,
            &current,
            || generation(original, &publisher.signed),
            || {
                let evidence = rolling_original::verify(
                    &publisher.api,
                    &publisher.receipt.repository.slug,
                    publisher.receipt.repository.id,
                    original,
                )?;
                generation(original, &evidence.signed)
            },
        )?;
        self.0.push(generation.clone());
        Ok(generation)
    }
}

fn select_original<T>(
    original: &RollingOriginal,
    current: &RollingOriginal,
    current_evidence: impl FnOnce() -> Result<T, OrchestratorError>,
    retained_evidence: impl FnOnce() -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    if original == current {
        current_evidence()
    } else {
        retained_evidence()
    }
}

fn generation(
    original: &RollingOriginal,
    signed: &crate::package_release_signer::VerifiedAttestedSignedPackage,
) -> Result<Generation, OrchestratorError> {
    let identity = local_identity(signed)?;
    let mut sizes = BTreeMap::new();
    for name in identity.assets.keys() {
        sizes.insert(
            name.clone(),
            read_regular(&signed.directory().join(name))?.len() as u64,
        );
    }
    Ok(Generation {
        original: original.clone(),
        identity,
        sizes,
        version: super::version::Version::from_receipt(signed.receipt())?,
    })
}

pub(super) fn bootstrap(snapshot: &RollingBootstrapSnapshot) -> Result<(), OrchestratorError> {
    let expected = env::var("VELNOR_PREVIEW_BOOTSTRAP_SNAPSHOT_SHA256")
        .map_err(|_| internal("rolling_bootstrap_review_required"))?;
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        || snapshot
            .sha256()
            .map_err(|_| internal("rolling_bootstrap_snapshot"))?
            != expected
    {
        return Err(internal("rolling_bootstrap_snapshot_mismatch"));
    }
    Ok(())
}

pub(super) fn history(
    publisher: &GitHubPackagePublisher,
    id: u64,
    envelope: &Envelope,
    live: &[RollingAsset],
    cache: &mut EvidenceCache,
) -> Result<Vec<RollingAsset>, OrchestratorError> {
    validate_chain(envelope)?;
    let mut visited = BTreeSet::new();
    authenticate(publisher, id, envelope, live, 0, &mut visited, cache)
}

fn validate_chain(mut envelope: &Envelope) -> Result<(), OrchestratorError> {
    let mut depth = 1;
    let mut receipts = BTreeSet::new();
    loop {
        if depth > MAX_HISTORY || !receipts.insert(&envelope.journal.identity.receipt_digest) {
            return Err(internal("rolling_predecessor_identity_or_bound"));
        }
        let Some(Predecessor::Native(previous)) = &envelope.predecessor else {
            return Ok(());
        };
        if previous.journal.identity.source_sha == envelope.journal.identity.source_sha {
            return Err(internal("rolling_predecessor_same_source_retry"));
        }
        depth += 1;
        envelope = previous;
    }
}

fn authenticate(
    publisher: &GitHubPackagePublisher,
    id: u64,
    envelope: &Envelope,
    live: &[RollingAsset],
    depth: usize,
    visited: &mut BTreeSet<String>,
    cache: &mut EvidenceCache,
) -> Result<Vec<RollingAsset>, OrchestratorError> {
    if depth >= MAX_HISTORY
        || envelope.schema != "velnor.rolling-preview-journal.v1"
        || envelope.repository != publisher.receipt.repository.slug
        || envelope.repository_id != publisher.receipt.repository.id
        || envelope.release_id != id
        || envelope.tag != "preview"
        || !visited.insert(envelope.journal.identity.receipt_digest.clone())
    {
        return Err(internal("rolling_predecessor_identity_or_bound"));
    }
    let generation = cache.verified(publisher, &envelope.journal.original)?;
    let identity = &generation.identity;
    if *identity != envelope.journal.identity {
        return Err(internal("rolling_predecessor_signed_identity"));
    }
    if identity != &publisher.identity {
        let candidate = super::version::Version::from_receipt(publisher.signed.receipt())?;
        if identity.source_sha == publisher.identity.source_sha
            || !candidate.supersedes(&generation.version)
        {
            return Err(internal("rolling_candidate_version_downgrade"));
        }
    }
    let predecessor = match &envelope.predecessor {
        Some(Predecessor::Bootstrap(snapshot)) => {
            if snapshot.release_id != id {
                return Err(internal("rolling_bootstrap_release_changed"));
            }
            bootstrap(snapshot)?;
            snapshot.assets.clone()
        }
        Some(Predecessor::Native(previous)) => {
            let old = cache.verified(publisher, &previous.journal.original)?;
            if !generation.version.supersedes(&old.version) {
                return Err(internal("rolling_predecessor_version_downgrade"));
            }
            if !super::observe::ancestor(
                &publisher.api,
                &previous.journal.identity.source_sha,
                &identity.source_sha,
            )? {
                return Err(internal("rolling_predecessor_source_rewind"));
            }
            authenticate(publisher, id, previous, live, depth + 1, visited, cache)?
        }
        None if envelope.journal.phase == RollingPhase::Committed
            && envelope.journal.obsolete.is_empty() =>
        {
            Vec::new()
        }
        None => return Err(internal("rolling_predecessor_missing")),
    };
    admitted_records(envelope, identity, &generation.sizes, &predecessor, live)
}

fn admitted_records(
    envelope: &Envelope,
    identity: &PublicationIdentity,
    sizes: &BTreeMap<String, u64>,
    predecessor: &[RollingAsset],
    live: &[RollingAsset],
) -> Result<Vec<RollingAsset>, OrchestratorError> {
    let journal = &envelope.journal;
    let obsolete_ids: BTreeSet<_> = journal.obsolete.iter().map(|asset| asset.id).collect();
    if obsolete_ids.len() != journal.obsolete.len()
        || journal
            .obsolete
            .iter()
            .any(|asset| !predecessor.contains(asset))
    {
        return Err(internal("rolling_predecessor_unowned_obsolete"));
    }
    let mut allowed = journal.obsolete.clone();
    if journal.phase == RollingPhase::Staging {
        if !journal.staged.is_empty() {
            return Err(internal("rolling_staging_ids_not_frozen"));
        }
        for asset in live {
            if identity.assets.iter().any(|(name, digest)| {
                asset.name == staged_name(identity, name)
                    && ((asset.state == RollingAssetState::Starter
                        && asset.size == 0
                        && asset.sha256.is_none())
                        || (asset.state == RollingAssetState::Uploaded
                            && asset.sha256.as_ref() == Some(digest)
                            && sizes.get(name) == Some(&asset.size)))
            }) {
                allowed.push(asset.clone());
            }
        }
    } else {
        if journal.staged.keys().ne(identity.assets.keys()) {
            return Err(internal("rolling_predecessor_staged_closure"));
        }
        let mut ids = BTreeSet::new();
        for (name, asset) in &journal.staged {
            if !ids.insert(asset.id)
                || asset.id == 0
                || obsolete_ids.contains(&asset.id)
                || asset.name != staged_name(identity, name)
                || asset.state != RollingAssetState::Uploaded
                || asset.sha256.as_ref() != identity.assets.get(name)
                || sizes.get(name) != Some(&asset.size)
            {
                return Err(internal("rolling_predecessor_staged_identity"));
            }
            allowed.push(asset.clone());
            let mut canonical = asset.clone();
            canonical.name.clone_from(name);
            allowed.push(canonical);
        }
    }
    Ok(allowed)
}

#[cfg(test)]
#[path = "package_release_rolling_backend_tests.rs"]
mod tests;

//! Fixed-release rolling adapter. Every mutation follows a fresh closed plan.

use super::{
    GitHubPackagePublisher, internal,
    rolling_state::{Envelope, Predecessor},
};
use crate::{
    OrchestratorError,
    package_publication::{PackagePublicationBackend, rolling::RollingPackagePublicationBackend},
};
use std::collections::BTreeSet;
use velnor_actions_contract::publication_retry::{
    PublicationIdentity,
    rolling::{RollingAction, RollingPhase, RollingState, rolling_action},
};

#[path = "package_release_rolling_backend_authority.rs"]
mod authority;
#[path = "package_release_rolling_backend_observe.rs"]
mod observe;
#[path = "package_release_rolling_backend_transport.rs"]
mod transport;
#[path = "package_release_rolling_backend_version.rs"]
mod version;

pub(super) struct RollingPublisher<'a> {
    publisher: &'a mut GitHubPackagePublisher,
    release_id: u64,
    evidence: authority::EvidenceCache,
}

impl<'a> RollingPublisher<'a> {
    pub(super) fn new(
        publisher: &'a mut GitHubPackagePublisher,
    ) -> Result<Self, OrchestratorError> {
        if publisher.tag != "preview" {
            return Err(internal("rolling_tag_not_preview"));
        }
        Ok(Self {
            publisher,
            release_id: super::published::configured_preview_release_id()?,
            evidence: authority::EvidenceCache::default(),
        })
    }

    fn write(&self, envelope: &Envelope) -> Result<(), OrchestratorError> {
        transport::write(&self.publisher.api, self.release_id, envelope)
    }

    fn commit(&mut self, mut envelope: Envelope) -> Result<(), OrchestratorError> {
        let publisher = &self.publisher;
        let mut release = observe::release(publisher, self.release_id)?;
        release.assets = publisher.api.complete_release_assets(self.release_id)?;
        if publisher.api.remote_assets(&release)? != publisher.identity.assets {
            return Err(internal("rolling_commit_actual_bytes"));
        }
        let expected = publisher.identity.clone();
        let fresh = self.observe()?;
        let available = self.available_original_assets()?;
        if fresh.journal.as_ref() != Some(&envelope.journal)
            || rolling_action(
                &expected,
                self.release_id,
                &fresh,
                self.candidate_current()?,
                &available,
            ) != RollingAction::Commit
        {
            return Err(internal("rolling_commit_state_changed"));
        }
        // The complete canonical IDs, tag and original are re-admitted by mutate
        // before this call. History is pruned only at this final verified marker.
        envelope.journal.phase = RollingPhase::Committed;
        envelope.journal.obsolete.clear();
        envelope.predecessor = None;
        self.write(&envelope)
    }
}

impl RollingPackagePublicationBackend for RollingPublisher<'_> {
    fn release_id(&self) -> u64 {
        self.release_id
    }

    fn authenticate_original(&mut self) -> Result<PublicationIdentity, OrchestratorError> {
        PackagePublicationBackend::authenticate_original(self.publisher)
    }

    fn observe(&mut self) -> Result<RollingState, OrchestratorError> {
        observe::state(self.publisher, self.release_id, &mut self.evidence)
    }

    fn available_original_assets(&mut self) -> Result<BTreeSet<String>, OrchestratorError> {
        PackagePublicationBackend::available_original_assets(self.publisher)
    }

    fn candidate_current(&mut self) -> Result<bool, OrchestratorError> {
        PackagePublicationBackend::candidate_current(self.publisher)
    }

    fn mutate(&mut self, action: RollingAction) -> Result<(), OrchestratorError> {
        let expected = self.authenticate_original()?;
        let state = self.observe()?;
        let available = self.available_original_assets()?;
        if rolling_action(
            &expected,
            self.release_id,
            &state,
            self.candidate_current()?,
            &available,
        ) != action
        {
            return Err(internal("rolling_mutation_observation_changed"));
        }
        let old = Envelope::parse(
            observe::release(self.publisher, self.release_id)?
                .body
                .as_deref(),
        )?;
        if old.as_ref().map(|item| &item.journal) != state.journal.as_ref() {
            return Err(internal("rolling_mutation_journal_changed"));
        }
        if let Some(previous) = &old {
            authority::history(
                self.publisher,
                self.release_id,
                previous,
                &state.assets,
                &mut self.evidence,
            )?;
        }
        match action {
            RollingAction::Begin(journal) => {
                let mut envelope = observe::envelope(self.publisher, self.release_id, journal);
                envelope.predecessor = Some(match old {
                    Some(previous) => Predecessor::Native(Box::new(previous)),
                    None => Predecessor::Bootstrap(observe::bootstrap(&state)),
                });
                authority::history(
                    self.publisher,
                    self.release_id,
                    &envelope,
                    &state.assets,
                    &mut self.evidence,
                )?;
                self.write(&envelope)
            }
            RollingAction::Ready(journal) => {
                let mut envelope = old.ok_or_else(|| internal("rolling_journal_missing"))?;
                envelope.journal = journal;
                self.write(&envelope)
            }
            RollingAction::Commit => {
                self.commit(old.ok_or_else(|| internal("rolling_journal_missing"))?)
            }
            mutation => transport::mutate(
                &self.publisher.api,
                self.release_id,
                &self.publisher.identity,
                self.publisher.signed.directory(),
                mutation,
            ),
        }
    }
}

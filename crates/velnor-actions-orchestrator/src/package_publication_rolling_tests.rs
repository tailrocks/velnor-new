//! Lost-response recovery and repeated freshness gates at every mutation.

use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::publication_retry::rolling::{
    RollingAsset, RollingAssetState, RollingOriginal, RollingPhase, RollingReleaseStatus,
};

struct Backend {
    identity: PublicationIdentity,
    state: RollingState,
    mutations: Vec<RollingAction>,
    lose_response_at: Option<usize>,
    stale_after: Option<usize>,
    authentications: usize,
    observations: usize,
}

impl Backend {
    fn new() -> Self {
        let old = RollingAsset {
            id: 1,
            name: "legacy.tar.gz".to_owned(),
            state: RollingAssetState::Uploaded,
            size: 100,
            sha256: Some("e".repeat(64)),
        };
        Self {
            identity: PublicationIdentity {
                source_sha: "a".repeat(40),
                receipt_digest: "b".repeat(64),
                assets: BTreeMap::from([
                    ("jackin.tar.gz".to_owned(), "c".repeat(64)),
                    ("release.json".to_owned(), "b".repeat(64)),
                    ("release.json.bundle".to_owned(), "d".repeat(64)),
                ]),
            },
            state: RollingState {
                original: RollingOriginal {
                    run_id: 101,
                    run_attempt: 1,
                    artifact_id: 102,
                    archive_sha256: "f".repeat(64),
                },
                release_id: 103,
                tag: "preview".to_owned(),
                status: RollingReleaseStatus::MutablePreview,
                tag_source: "0".repeat(40),
                tag_can_fast_forward: true,
                assets: vec![old.clone()],
                replaceable: vec![old],
                journal: None,
                published: None,
                journal_can_advance: false,
            },
            mutations: Vec::new(),
            lose_response_at: None,
            stale_after: None,
            authentications: 0,
            observations: 0,
        }
    }

    fn apply(&mut self, action: &RollingAction) {
        match action {
            RollingAction::Begin(journal) | RollingAction::Ready(journal) => {
                self.state.journal = Some(journal.clone());
            }
            RollingAction::UploadStage(name) => {
                let id = self
                    .state
                    .assets
                    .iter()
                    .map(|asset| asset.id)
                    .max()
                    .unwrap_or(0)
                    + 1;
                self.state.assets.push(RollingAsset {
                    id,
                    name: velnor_actions_contract::publication_retry::rolling::staged_name(
                        &self.identity,
                        name,
                    ),
                    state: RollingAssetState::Uploaded,
                    size: 100,
                    sha256: Some(self.identity.assets[name].clone()),
                });
            }
            RollingAction::DeleteStarter(id) | RollingAction::DeleteObsolete(id) => {
                self.state.assets.retain(|asset| asset.id != *id);
            }
            RollingAction::Rename { id, name } => self
                .state
                .assets
                .iter_mut()
                .find(|asset| asset.id == *id)
                .expect("captured ID")
                .name
                .clone_from(name),
            RollingAction::AdvanceTag => {
                self.state.tag_source.clone_from(&self.identity.source_sha);
            }
            RollingAction::Commit => {
                self.state.journal.as_mut().expect("journal").phase = RollingPhase::Committed;
            }
            RollingAction::Reconcile | RollingAction::Reject(_) => {
                panic!("terminal actions never reach transport")
            }
        }
    }
}

impl RollingPackagePublicationBackend for Backend {
    fn release_id(&self) -> u64 {
        103
    }
    fn authenticate_original(&mut self) -> Result<PublicationIdentity, OrchestratorError> {
        self.authentications += 1;
        Ok(self.identity.clone())
    }
    fn observe(&mut self) -> Result<RollingState, OrchestratorError> {
        self.observations += 1;
        Ok(self.state.clone())
    }
    fn available_original_assets(&mut self) -> Result<BTreeSet<String>, OrchestratorError> {
        Ok(self.identity.assets.keys().cloned().collect())
    }
    fn candidate_current(&mut self) -> Result<bool, OrchestratorError> {
        Ok(self
            .stale_after
            .is_none_or(|limit| self.mutations.len() < limit))
    }
    fn mutate(&mut self, action: RollingAction) -> Result<(), OrchestratorError> {
        self.apply(&action);
        self.mutations.push(action);
        if self.lose_response_at == Some(self.mutations.len()) {
            self.lose_response_at = None;
            return Err(internal("simulated_accepted_mutation_lost_response"));
        }
        Ok(())
    }
}

#[test]
fn every_ambiguous_mutation_stops_then_reobserves_before_recovery() {
    let mut complete = Backend::new();
    assert_eq!(
        publish_rolling_package_release(&mut complete).expect("publication"),
        PackagePublicationOutcome::Published
    );
    let count = complete.mutations.len();
    assert_eq!(count, 11);
    for interruption in 1..=count {
        let mut backend = Backend::new();
        backend.lose_response_at = Some(interruption);
        let error =
            publish_rolling_package_release(&mut backend).expect_err("ambiguous response stops");
        assert!(
            error
                .to_string()
                .contains("simulated_accepted_mutation_lost_response")
        );
        assert_eq!(backend.mutations.len(), interruption);
        let observations = backend.observations;
        publish_rolling_package_release(&mut backend).expect("independent recovery");
        assert!(backend.observations > observations);
        assert_eq!(
            backend.mutations.len(),
            count,
            "no accepted operation repeated"
        );
        assert!(backend.authentications > backend.mutations.len());
    }
}

#[test]
fn source_advancement_stops_without_cleanup_or_tag_movement() {
    for limit in 0..11 {
        let mut backend = Backend::new();
        backend.stale_after = Some(limit);
        let error = publish_rolling_package_release(&mut backend).expect_err("stale candidate");
        assert!(error.to_string().contains("StaleCandidate"));
        assert_eq!(backend.mutations.len(), limit);
    }
}

#[test]
fn unrelated_asset_is_never_a_delete_candidate() {
    let mut backend = Backend::new();
    backend.state.assets.push(RollingAsset {
        id: 999,
        name: "foreign.zip".to_owned(),
        state: RollingAssetState::Uploaded,
        size: 100,
        sha256: Some("e".repeat(64)),
    });
    assert!(
        publish_rolling_package_release(&mut backend)
            .expect_err("unknown predecessor")
            .to_string()
            .contains("UnadmittedPredecessor")
    );
    assert_eq!(backend.mutations, Vec::<RollingAction>::new());
}

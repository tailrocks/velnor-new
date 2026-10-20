use super::*;
use std::collections::BTreeMap;

struct Backend {
    identity: PublicationIdentity,
    state: PublicationState,
    current: bool,
    mutations: Vec<String>,
    interrupt_after: Option<usize>,
    stale_after: Option<usize>,
    gate_calls: usize,
}

impl Backend {
    fn new() -> Self {
        Self {
            identity: PublicationIdentity {
                source_sha: "a".repeat(40),
                receipt_digest: "b3:original".to_owned(),
                assets: BTreeMap::from([
                    ("payload.tar.gz".to_owned(), "1".repeat(64)),
                    ("release.json".to_owned(), "2".repeat(64)),
                    ("release.json.bundle".to_owned(), "3".repeat(64)),
                ]),
            },
            state: PublicationState::Absent,
            current: true,
            mutations: Vec::new(),
            interrupt_after: None,
            stale_after: None,
            gate_calls: 0,
        }
    }

    fn mutated(&mut self, name: &str) -> Result<(), OrchestratorError> {
        self.mutations.push(name.to_owned());
        if self.stale_after == Some(self.mutations.len()) {
            self.current = false;
        }
        if self.interrupt_after == Some(self.mutations.len()) {
            Err(internal("transport_interrupted_after_remote_success"))
        } else {
            Ok(())
        }
    }
}

impl PackagePublicationBackend for Backend {
    fn authenticate_original(&mut self) -> Result<PublicationIdentity, OrchestratorError> {
        Ok(self.identity.clone())
    }
    fn observe(&mut self) -> Result<PublicationState, OrchestratorError> {
        Ok(self.state.clone())
    }
    fn available_original_assets(&mut self) -> Result<BTreeSet<String>, OrchestratorError> {
        Ok(self.identity.assets.keys().cloned().collect())
    }
    fn candidate_current(&mut self) -> Result<bool, OrchestratorError> {
        self.gate_calls += 1;
        Ok(self.current)
    }
    fn create_tag(&mut self) -> Result<(), OrchestratorError> {
        if !self.candidate_current()? {
            return Err(internal("package_publication_refused:StaleCandidate"));
        }
        assert_eq!(self.state, PublicationState::Absent);
        self.state = PublicationState::TagOnly {
            source_sha: self.identity.source_sha.clone(),
            original_receipt_digest: Some(self.identity.receipt_digest.clone()),
        };
        self.mutated("tag")
    }
    fn create_draft(&mut self) -> Result<(), OrchestratorError> {
        if !self.candidate_current()? {
            return Err(internal("package_publication_refused:StaleCandidate"));
        }
        assert!(matches!(self.state, PublicationState::TagOnly { .. }));
        self.state = PublicationState::Draft {
            source_sha: self.identity.source_sha.clone(),
            original_receipt_digest: Some(self.identity.receipt_digest.clone()),
            assets: BTreeMap::new(),
        };
        self.mutated("draft")
    }
    fn upload_original_asset(&mut self, name: &str) -> Result<(), OrchestratorError> {
        if !self.candidate_current()? {
            return Err(internal("package_publication_refused:StaleCandidate"));
        }
        let PublicationState::Draft { assets, .. } = &mut self.state else {
            panic!("draft required")
        };
        assert!(
            assets
                .insert(name.to_owned(), self.identity.assets[name].clone())
                .is_none()
        );
        self.mutated(name)
    }
    fn publish_draft(&mut self) -> Result<(), OrchestratorError> {
        if !self.candidate_current()? {
            return Err(internal("package_publication_refused:StaleCandidate"));
        }
        let PublicationState::Draft { assets, .. } = &self.state else {
            panic!("draft required")
        };
        assert_eq!(assets, &self.identity.assets);
        self.state = PublicationState::Published {
            identity: self.identity.clone(),
            immutable: true,
        };
        self.mutated("publish")
    }
}

#[test]
fn publication_rechecks_every_mutation_and_verifies_terminal_state() {
    let mut backend = Backend::new();
    assert_eq!(
        publish_package_release(&mut backend).expect("publication"),
        PackagePublicationOutcome::Published
    );
    assert_eq!(
        backend.mutations,
        [
            "tag",
            "draft",
            "release.json",
            "release.json.bundle",
            "payload.tar.gz",
            "publish"
        ]
    );
    assert_eq!(backend.gate_calls, 6);
    backend.current = false;
    assert_eq!(
        publish_package_release(&mut backend).expect("reconcile"),
        PackagePublicationOutcome::AlreadyPublished
    );
    assert_eq!(backend.mutations.len(), 6);
}

#[test]
fn interrupted_success_resumes_without_replacing_any_remote_object() {
    for boundary in 1..=6 {
        let mut backend = Backend::new();
        backend.interrupt_after = Some(boundary);
        assert!(publish_package_release(&mut backend).is_err());
        backend.interrupt_after = None;
        publish_package_release(&mut backend).expect("authenticated original retry");
        assert_eq!(backend.mutations.len(), 6, "boundary {boundary}");
        let unique: BTreeSet<_> = backend.mutations.iter().collect();
        assert_eq!(unique.len(), 6, "no overwrite at boundary {boundary}");
    }
}

#[test]
fn stale_after_any_unfinished_mutation_blocks_further_writes() {
    for boundary in 1..=5 {
        let mut backend = Backend::new();
        backend.stale_after = Some(boundary);
        assert!(publish_package_release(&mut backend).is_err());
        assert_eq!(backend.mutations.len(), boundary);
    }
}

#[test]
fn conflict_or_mutable_publication_blocks_reconciliation() {
    for immutable in [false, true] {
        let mut backend = Backend::new();
        let mut identity = backend.identity.clone();
        if immutable {
            identity.receipt_digest = "b3:conflict".to_owned();
        }
        backend.state = PublicationState::Published {
            identity,
            immutable,
        };
        assert!(publish_package_release(&mut backend).is_err());
        assert_eq!(backend.mutations, [] as [std::string::String; 0]);
    }
}

#[test]
fn complete_twenty_nine_asset_stable_transaction_has_one_guard_per_write() {
    let mut backend = Backend::new();
    for n in 0..26 {
        backend
            .identity
            .assets
            .insert(format!("payload-{n:02}.tar.gz"), "d".repeat(64));
    }
    backend.state = PublicationState::TagOnly {
        source_sha: backend.identity.source_sha.clone(),
        original_receipt_digest: Some(backend.identity.receipt_digest.clone()),
    };
    assert_eq!(backend.identity.assets.len(), 29);
    publish_package_release(&mut backend).expect("full stable transaction");
    assert_eq!(backend.mutations.len(), 31);
    assert_eq!(
        backend.gate_calls, 31,
        "every transport operation owns exactly one fresh guard"
    );
}

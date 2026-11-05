// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, Digest, Github, JournalIo, JournalRegistryFake, JournalTag, Registry, Sha256,
    fixture_archive, fixture_journal_plan, fixture_no_baseline, fs, journal_existing_owners, json,
    make_fixture, prepare_crate_release, test_git, test_hex, velnor_actions_orchestrator,
};

#[test]
pub(super) fn journal_observation_runtime_is_blocked_until_authority_is_qualified() {
    for operation in [
        "journal-observe",
        "journal-coordinate",
        "journal-start",
        "journal-repair-tag",
        "journal-repair-release",
        "journal-upload-intent",
        "journal-recovery-inspect",
        "journal-reconcile-evidence",
    ] {
        let error = crate::crate_release_runtime::run_crate_release(operation)
            .expect_err("guard must reject");
        assert!(
            error
                .to_string()
                .contains("crate_release_authority_not_qualified")
        );
    }
}

#[test]
pub(super) fn prior_upload_recovery_reobserves_registry_without_writes()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation;
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let mut admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, mut current) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let mut previous = current.clone();
    previous.identity.run_id = 11;
    previous.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    current.predecessor = Some(previous.identity.clone());
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: previous.identity.run_id,
        run_attempt: previous.identity.run_attempt,
        workflow_sha: previous.identity.workflow_sha.clone(),
    });
    admission.intent = intent.clone();
    current.intent_sha256 = test_hex(&Sha256::digest(
        velnor_actions_contract::canonical_json_bytes(&intent)?,
    ));
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &previous.identity, &previous, root, test_git())?;
    record_journal_intent(
        &journal,
        &previous.identity,
        &previous,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    record_source_bound_start(&journal, &identity, &current, root, test_git())?;
    let input = CrateReleaseObservation {
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &current,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 2,
    };
    assert_prior_observation_states(&input, &previous)?;
    Ok(())
}

pub(super) fn assert_prior_observation_states(
    input: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    previous: &velnor_actions_orchestrator::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        RecoveryState, observe_prior_recovery,
    };
    let before = input.journal.refs.borrow().len();
    let missing = observe_prior_recovery(input, &previous.identity)?;
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].state, RecoveryState::Missing);
    assert_eq!(
        serde_json::to_value(&missing[0])?["prior_record"],
        "intention-only"
    );
    assert_eq!(input.journal.refs.borrow().len(), before);
    *input.registry.archive.borrow_mut() = fixture_archive(input.source)?;
    input.registry.absent.set(false);
    let present = observe_prior_recovery(input, &previous.identity)?;
    assert_eq!(present[0].state, RecoveryState::Present);
    assert_eq!(input.journal.refs.borrow().len(), before);
    append_fixture_prior_upload_observation(input.journal, previous)?;
    let before = input.journal.refs.borrow().len();
    let recorded = observe_prior_recovery(input, &previous.identity)?;
    assert_eq!(
        serde_json::to_value(&recorded[0])?["prior_record"],
        "observation-recorded"
    );
    assert_eq!(recorded[0].state, RecoveryState::Present);
    assert_eq!(input.journal.refs.borrow().len(), before);
    input.github.approved.set(false);
    assert!(observe_prior_recovery(input, &previous.identity).is_err());
    input.github.approved.set(true);
    input.registry.archive.borrow_mut().push(0);
    assert!(observe_prior_recovery(input, &previous.identity).is_err());
    assert_eq!(input.journal.refs.borrow().len(), before);
    Ok(())
}

pub(super) fn append_fixture_prior_upload_observation(
    journal: &JournalRegistryFake,
    receipt: &velnor_actions_orchestrator::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalFile, JournalFileKind, JournalMutation, JournalMutationKind, JournalMutationPhase,
        JournalMutationResult,
    };
    let mutation = JournalMutation {
        semver_evidence: None,
        sequence: 2,
        package: "demo".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        phase: JournalMutationPhase::Observed {
            result: JournalMutationResult::Upload {
                registry_checksum: "d".repeat(64),
                downloaded_checksum: "d".repeat(64),
                owners: journal_existing_owners(&receipt.packages[0].owners),
                features: BTreeMap::new(),
                files: vec![JournalFile {
                    path: "src/lib.rs".into(),
                    sha256: "e".repeat(64),
                    kind: JournalFileKind::Regular { executable: false },
                }],
            },
        },
    };
    let body = velnor_actions_contract::canonical_json_bytes(
        &json!({"schema":1,"identity":receipt.identity,"receipt":receipt,"mutation":mutation}),
    )?;
    let repository = &receipt.identity.repository;
    let blob = journal.create_blob(repository, &body)?;
    let name = format!(
        "velnor-receipt/{}/{}/operation-2",
        receipt.identity.run_id, receipt.identity.run_attempt
    );
    let tag = JournalTag {
        name: name.clone(),
        object_type: "blob".into(),
        object_sha: blob,
        message: "velnor-release-receipt-v1".into(),
    };
    let tag_sha = journal.create_tag_object(repository, &tag)?;
    journal.create_ref(repository, &format!("refs/tags/{name}"), &tag_sha)?;
    Ok(())
}

pub(super) fn verify_reconciled_candidate(
    input: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    github: &Github,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::derive_reconciled_candidate;
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
    };
    let before = input.journal.refs.borrow().len();
    let mut wrong_identity = input.identity.clone();
    wrong_identity.run_id += 1;
    let wrong = velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation {
        identity: &wrong_identity,
        ..*input
    };
    assert!(derive_reconciled_candidate(&wrong).is_err());
    let mut wrong_intent = input.intent.clone();
    wrong_intent.rust_version = "1.0.0".into();
    let wrong = velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation {
        intent: &wrong_intent,
        ..*input
    };
    assert!(derive_reconciled_candidate(&wrong).is_err());
    github.owner_match.set(false);
    assert!(derive_reconciled_candidate(input).is_err());
    github.owner_match.set(true);
    fs::write(input.source.join("src/lib.rs"), "tampered")?;
    assert!(derive_reconciled_candidate(input).is_err());
    fs::write(input.source.join("src/lib.rs"), "pub fn demo() {}")?;
    let candidate = derive_reconciled_candidate(input)?;
    assert_eq!(candidate.history.len(), 6);
    assert_eq!(candidate.packages.len(), 1);
    assert_eq!(candidate.packages[0].release_id, 12);
    assert_eq!(
        candidate.packages[0].registry.features.get("demo-feature"),
        Some(&Vec::new())
    );
    assert!(
        candidate.packages[0]
            .source_inventory
            .contains_key("src/lib.rs")
    );
    assert_eq!(input.journal.refs.borrow().len(), before);
    github.approved.set(false);
    assert!(derive_reconciled_candidate(input).is_err());
    github.approved.set(true);
    github.tag_wrong.set(true);
    assert!(derive_reconciled_candidate(input).is_err());
    github.tag_wrong.set(false);
    github.release_variant.set(5);
    assert!(derive_reconciled_candidate(input).is_err());
    github.release_variant.set(0);
    input.registry.archive.borrow_mut().push(0);
    assert!(derive_reconciled_candidate(input).is_err());
    input.registry.archive.borrow_mut().pop();
    assert_eq!(input.journal.refs.borrow().len(), before);
    record_journal_intent(
        input.journal,
        input.identity,
        input.journal_receipt,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 7,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    assert!(derive_reconciled_candidate(input).is_err());
    Ok(())
}

#[test]
pub(super) fn reconciled_candidate_allows_verified_state_without_mutation_history()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        CrateReleaseObservation, derive_reconciled_candidate,
    };
    use velnor_actions_orchestrator::crate_release_journal::record_source_bound_start;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.tag_exists.set(true);
    let input = CrateReleaseObservation {
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 0,
    };
    let before = journal.refs.borrow().len();
    let candidate = derive_reconciled_candidate(&input)?;
    assert_eq!(candidate.history.len(), 0);
    assert_eq!(candidate.packages.len(), 1);
    assert_eq!(candidate.packages[0].release_id, 12);
    assert_eq!(journal.refs.borrow().len(), before);
    Ok(())
}

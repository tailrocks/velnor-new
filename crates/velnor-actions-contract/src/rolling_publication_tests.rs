//! Failure and recovery proofs for the rolling transaction planner.

use super::*;

fn identity() -> PublicationIdentity {
    PublicationIdentity {
        source_sha: "a".repeat(40),
        receipt_digest: "b".repeat(64),
        assets: BTreeMap::from([
            ("jackin.tar.gz".to_owned(), "c".repeat(64)),
            ("release.json".to_owned(), "b".repeat(64)),
            ("release.json.bundle".to_owned(), "d".repeat(64)),
        ]),
    }
}

fn asset(id: u64, name: &str, digest: &str) -> RollingAsset {
    RollingAsset {
        id,
        name: name.to_owned(),
        state: RollingAssetState::Uploaded,
        size: 100,
        sha256: Some(digest.to_owned()),
    }
}

fn state() -> RollingState {
    let old = asset(1, "legacy.tar.gz", &"e".repeat(64));
    RollingState {
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
    }
}

fn action(observed: &RollingState) -> RollingAction {
    rolling_action(
        &identity(),
        103,
        observed,
        true,
        &identity().assets.into_keys().collect(),
    )
}

fn stage_all(observed: &mut RollingState) {
    let RollingAction::Begin(journal) = action(observed) else {
        panic!("intent must precede upload");
    };
    observed.journal = Some(journal);
    for (index, (name, digest)) in identity().assets.iter().enumerate() {
        observed.assets.push(asset(
            u64::try_from(index).expect("bounded index") + 10,
            &staged_name(&identity(), name),
            digest,
        ));
    }
}

#[test]
fn durable_intent_precedes_upload_and_freezes_ids_before_delete() {
    let mut observed = state();
    let RollingAction::Begin(journal) = action(&observed) else {
        panic!("intent required");
    };
    assert_eq!(journal.phase, RollingPhase::Staging);
    assert!(journal.staged.is_empty());
    observed.journal = Some(journal);
    assert_eq!(
        action(&observed),
        RollingAction::UploadStage("jackin.tar.gz".to_owned())
    );
    observed = state();
    stage_all(&mut observed);
    let RollingAction::Ready(journal) = action(&observed) else {
        panic!("complete stages must freeze before deletion");
    };
    assert_eq!(journal.phase, RollingPhase::Replacing);
    assert_eq!(journal.staged.len(), 3);
    observed.journal = Some(journal);
    assert_eq!(action(&observed), RollingAction::DeleteObsolete(1));
}

#[test]
fn unknown_assets_never_gain_deletion_authority() {
    let mut observed = state();
    observed
        .assets
        .push(asset(2, "unrelated.zip", &"e".repeat(64)));
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::UnadmittedPredecessor)
    );
    observed = state();
    stage_all(&mut observed);
    observed
        .assets
        .push(asset(2, "unrelated.zip", &"e".repeat(64)));
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::UnadmittedPredecessor)
    );
}

#[test]
fn interrupted_upload_reobserves_exact_bytes_and_only_own_starter() {
    let mut observed = state();
    stage_all(&mut observed);
    let name = staged_name(&identity(), "jackin.tar.gz");
    let item = observed
        .assets
        .iter_mut()
        .find(|item| item.name == name)
        .expect("stage");
    item.state = RollingAssetState::Starter;
    item.size = 0;
    item.sha256 = None;
    assert_eq!(action(&observed), RollingAction::DeleteStarter(10));
    let item = observed
        .assets
        .iter_mut()
        .find(|item| item.id == 10)
        .expect("starter");
    item.state = RollingAssetState::Uploaded;
    item.size = 100;
    item.sha256 = Some("e".repeat(64));
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::ConflictingAsset)
    );
}

#[test]
fn receipt_is_last_and_tag_advances_before_its_rename() {
    let mut observed = state();
    stage_all(&mut observed);
    let RollingAction::Ready(journal) = action(&observed) else {
        panic!("ready")
    };
    observed.journal = Some(journal);
    observed.assets.retain(|item| item.id != 1);
    for name in ["jackin.tar.gz", "release.json.bundle"] {
        let RollingAction::Rename {
            id,
            name: actual_name,
        } = action(&observed)
        else {
            panic!("rename")
        };
        assert_eq!(actual_name, name);
        observed
            .assets
            .iter_mut()
            .find(|item| item.id == id)
            .expect("stage")
            .name = actual_name;
    }
    assert_eq!(action(&observed), RollingAction::AdvanceTag);
    observed.tag_source = identity().source_sha;
    let RollingAction::Rename { id, name } = action(&observed) else {
        panic!("receipt rename")
    };
    assert_eq!(name, "release.json");
    observed
        .assets
        .iter_mut()
        .find(|item| item.id == id)
        .expect("receipt")
        .name = name;
    assert_eq!(action(&observed), RollingAction::Commit);
    observed.journal.as_mut().expect("journal").phase = RollingPhase::Committed;
    assert_eq!(action(&observed), RollingAction::Reconcile);
    observed.tag_source = "0".repeat(40);
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::JournalChanged)
    );
}

#[test]
fn stale_candidates_and_immutable_releases_never_mutate() {
    let mut observed = state();
    assert_eq!(
        rolling_action(&identity(), 103, &observed, false, &BTreeSet::new()),
        RollingAction::Reject(RollingRejection::StaleCandidate)
    );
    observed.status = RollingReleaseStatus::Immutable;
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::ImmutableOrDraft)
    );
    observed.status = RollingReleaseStatus::MutablePreview;
    observed.tag_can_fast_forward = false;
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::TagWouldRewind)
    );
}

#[test]
fn replacing_detects_metadata_changes_and_forged_predecessor_ids() {
    let mut observed = state();
    stage_all(&mut observed);
    let RollingAction::Ready(journal) = action(&observed) else {
        panic!("ready")
    };
    observed.journal = Some(journal);
    observed.assets[0].name = "changed.tar.gz".to_owned();
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::JournalChanged)
    );
    observed.assets[0] = observed.replaceable[0].clone();
    observed
        .journal
        .as_mut()
        .expect("journal")
        .obsolete
        .push(asset(999, "forged.zip", &"e".repeat(64)));
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::JournalChanged)
    );
}

#[test]
fn original_pointer_and_same_source_conflicts_fail_closed() {
    let mut observed = state();
    stage_all(&mut observed);
    observed.original.artifact_id += 1;
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::ConflictingRetry)
    );
    observed = state();
    let mut prior = identity();
    prior.receipt_digest = "e".repeat(64);
    observed.published = Some(prior);
    assert_eq!(
        action(&observed),
        RollingAction::Reject(RollingRejection::ConflictingRetry)
    );
}

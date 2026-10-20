//! Deletion closure rejects forged IDs, bytes and incomplete phase transitions.

use super::*;
use velnor_actions_contract::publication_retry::rolling::{RollingJournal, RollingOriginal};

fn fixture() -> (
    Envelope,
    PublicationIdentity,
    BTreeMap<String, u64>,
    RollingAsset,
) {
    let identity = PublicationIdentity {
        source_sha: "a".repeat(40),
        receipt_digest: "b".repeat(64),
        assets: BTreeMap::from([("release.json".to_owned(), "b".repeat(64))]),
    };
    let asset = RollingAsset {
        id: 7,
        name: staged_name(&identity, "release.json"),
        state: RollingAssetState::Uploaded,
        size: 100,
        sha256: Some("b".repeat(64)),
    };
    let envelope = Envelope {
        schema: "velnor.rolling-preview-journal.v1".to_owned(),
        repository: "example/app".to_owned(),
        repository_id: 1,
        release_id: 2,
        tag: "preview".to_owned(),
        journal: RollingJournal {
            identity: identity.clone(),
            original: RollingOriginal {
                run_id: 3,
                run_attempt: 1,
                artifact_id: 4,
                archive_sha256: "c".repeat(64),
            },
            phase: RollingPhase::Replacing,
            staged: BTreeMap::from([("release.json".to_owned(), asset.clone())]),
            obsolete: Vec::new(),
        },
        predecessor: None,
    };
    (
        envelope,
        identity,
        BTreeMap::from([("release.json".to_owned(), 100)]),
        asset,
    )
}

#[test]
fn captured_ids_allow_only_signed_stage_and_canonical_names() {
    let (envelope, identity, sizes, asset) = fixture();
    let admitted =
        admitted_records(&envelope, &identity, &sizes, &[], &[]).expect("signed staged tuple");
    assert!(admitted.contains(&asset));
    let mut canonical = asset.clone();
    canonical.name = "release.json".to_owned();
    assert!(admitted.contains(&canonical));
    for changed in [
        RollingAsset {
            id: 8,
            ..canonical.clone()
        },
        RollingAsset {
            size: 101,
            ..canonical.clone()
        },
        RollingAsset {
            sha256: Some("d".repeat(64)),
            ..canonical.clone()
        },
        RollingAsset {
            name: "foreign.json".to_owned(),
            ..canonical
        },
    ] {
        assert!(!admitted.contains(&changed));
    }
}

#[test]
fn obsolete_requires_exact_authenticated_predecessor_tuple() {
    let (mut envelope, identity, sizes, asset) = fixture();
    let old = RollingAsset {
        id: 9,
        name: "old.tar.gz".to_owned(),
        ..asset
    };
    envelope.journal.obsolete = vec![old.clone()];
    assert!(admitted_records(&envelope, &identity, &sizes, &[], &[]).is_err());
    assert!(
        admitted_records(
            &envelope,
            &identity,
            &sizes,
            std::slice::from_ref(&old),
            &[]
        )
        .is_ok()
    );
    envelope.journal.obsolete[0].size += 1;
    assert!(admitted_records(&envelope, &identity, &sizes, &[old], &[]).is_err());
}

#[test]
fn replacing_rejects_incomplete_ids_digest_size_and_overlap() {
    let (envelope, identity, sizes, asset) = fixture();
    let mut cases = Vec::new();
    let mut changed = envelope.clone();
    changed.journal.staged.clear();
    cases.push(changed);
    let mut changed = envelope.clone();
    changed
        .journal
        .staged
        .get_mut("release.json")
        .expect("asset")
        .size += 1;
    cases.push(changed);
    let mut changed = envelope.clone();
    changed
        .journal
        .staged
        .get_mut("release.json")
        .expect("asset")
        .sha256 = Some("d".repeat(64));
    cases.push(changed);
    let mut changed = envelope;
    changed.journal.obsolete.push(asset.clone());
    cases.push(changed);
    for changed in cases {
        assert!(
            admitted_records(
                &changed,
                &identity,
                &sizes,
                std::slice::from_ref(&asset),
                &[]
            )
            .is_err()
        );
    }
}

#[test]
fn staging_only_admits_own_exact_stage_or_unfinished_starter() {
    let (mut envelope, identity, sizes, asset) = fixture();
    envelope.journal.phase = RollingPhase::Staging;
    assert!(admitted_records(&envelope, &identity, &sizes, &[], &[]).is_err());
    envelope.journal.staged.clear();
    let starter = RollingAsset {
        state: RollingAssetState::Starter,
        size: 0,
        sha256: None,
        ..asset.clone()
    };
    let foreign = RollingAsset {
        name: "other-generation".to_owned(),
        ..asset.clone()
    };
    let wrong_size = RollingAsset {
        size: 101,
        ..asset.clone()
    };
    let admitted = admitted_records(
        &envelope,
        &identity,
        &sizes,
        &[],
        &[starter.clone(), foreign.clone(), wrong_size.clone()],
    )
    .expect("staging classification");
    assert_eq!(admitted, vec![starter]);
    assert!(!admitted.contains(&foreign));
    assert!(!admitted.contains(&wrong_size));
}

#[test]
fn active_current_run_uses_exact_reauthenticated_signed_original() {
    let (envelope, _, _, _) = fixture();
    let current = envelope.journal.original;
    // The retained verifier requires a completed original workflow, whereas the
    // current publication job is still running. Exact pointer equality alone
    // selects the independently reauthenticated current signed package.
    assert_eq!(
        select_original(
            &current,
            &current,
            || Ok(7),
            || Err(internal("completed_workflow_required"))
        )
        .expect("active original"),
        7
    );
    for changed in [
        RollingOriginal {
            run_id: 99,
            ..current.clone()
        },
        RollingOriginal {
            run_attempt: 2,
            ..current.clone()
        },
        RollingOriginal {
            artifact_id: 99,
            ..current.clone()
        },
        RollingOriginal {
            archive_sha256: "d".repeat(64),
            ..current.clone()
        },
    ] {
        assert!(
            select_original(
                &changed,
                &current,
                || panic!("a different original must not reuse current evidence"),
                || Err::<u64, _>(internal("completed_workflow_required"))
            )
            .is_err()
        );
    }
}

fn chain(length: usize) -> Envelope {
    let (mut result, _, _, _) = fixture();
    for index in 0..length {
        let (mut next, _, _, _) = fixture();
        next.journal.identity.source_sha = format!("{index:040x}");
        next.journal.identity.receipt_digest = format!("{index:064x}");
        next.journal.original.run_id += index as u64;
        next.journal.original.artifact_id += index as u64;
        next.predecessor = (index > 0).then(|| Predecessor::Native(Box::new(result)));
        result = next;
    }
    result
}

#[test]
fn ninth_generation_is_rejected_before_remote_authority_or_begin_patch() {
    assert!(validate_chain(&chain(8)).is_ok());
    assert!(validate_chain(&chain(9)).is_err());
}

#[test]
fn nested_distinct_originals_at_one_source_cannot_be_adopted() {
    let mut forged = chain(2);
    let Some(Predecessor::Native(previous)) = &mut forged.predecessor else {
        panic!("previous generation");
    };
    assert_ne!(previous.journal.original, forged.journal.original);
    assert_ne!(
        previous.journal.identity.receipt_digest,
        forged.journal.identity.receipt_digest
    );
    previous
        .journal
        .identity
        .source_sha
        .clone_from(&forged.journal.identity.source_sha);
    assert!(validate_chain(&forged).is_err());
}

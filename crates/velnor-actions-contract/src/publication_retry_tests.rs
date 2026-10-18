//! Retry transition coverage, including every partial upload boundary.
use super::*;

fn identity() -> PublicationIdentity {
    PublicationIdentity {
        source_sha: "a".repeat(40),
        receipt_digest: "b3-".to_owned() + &"b".repeat(64),
        assets: BTreeMap::from([
            ("payload.tar.gz".to_owned(), "c".repeat(64)),
            ("payload.tar.gz.bundle".to_owned(), "d".repeat(64)),
            ("release.json".to_owned(), "e".repeat(64)),
            ("release.json.bundle".to_owned(), "f".repeat(64)),
        ]),
    }
}

#[test]
fn absent_and_tag_only_resume_without_moving_tag() {
    let expected = identity();
    let available = expected.assets.keys().cloned().collect();
    assert_eq!(
        publication_action(&expected, &PublicationState::Absent, true, &available),
        PublicationAction::CreateTagAndDraft
    );
    let tag = PublicationState::TagOnly {
        source_sha: expected.source_sha.clone(),
        original_receipt_digest: Some(expected.receipt_digest.clone()),
    };
    assert_eq!(
        publication_action(&expected, &tag, true, &available),
        PublicationAction::CreateDraft
    );
    let unbound_tag = PublicationState::TagOnly {
        source_sha: expected.source_sha.clone(),
        original_receipt_digest: None,
    };
    assert_eq!(
        publication_action(&expected, &unbound_tag, true, &available),
        PublicationAction::Reject(PublicationRejection::UnknownPublicationOrigin)
    );
    let conflicting_tag = PublicationState::TagOnly {
        source_sha: expected.source_sha.clone(),
        original_receipt_digest: Some("different-receipt".to_owned()),
    };
    assert_eq!(
        publication_action(&expected, &conflicting_tag, true, &available),
        PublicationAction::Reject(PublicationRejection::ConflictingIdentity)
    );
    let conflict = PublicationState::TagOnly {
        source_sha: "0".repeat(40),
        original_receipt_digest: Some(expected.receipt_digest.clone()),
    };
    assert_eq!(
        publication_action(&expected, &conflict, true, &available),
        PublicationAction::Reject(PublicationRejection::ConflictingTag)
    );
}

#[test]
fn every_interrupted_upload_resumes_only_missing_original_assets() {
    let expected = identity();
    let available = expected.assets.keys().cloned().collect();
    let upload_order = super::ordered_asset_names(&expected);
    for uploaded_count in 0..=expected.assets.len() {
        let uploaded: BTreeMap<_, _> = upload_order
            .iter()
            .take(uploaded_count)
            .map(|name| (name.clone(), expected.assets[name].clone()))
            .collect();
        let missing_assets: Vec<_> = upload_order
            .iter()
            .filter(|name| !uploaded.contains_key(*name))
            .cloned()
            .collect();
        let draft = PublicationState::Draft {
            source_sha: expected.source_sha.clone(),
            original_receipt_digest: Some(expected.receipt_digest.clone()),
            assets: uploaded,
        };
        assert_eq!(
            publication_action(&expected, &draft, true, &available),
            PublicationAction::CompleteDraft { missing_assets }
        );
        assert_eq!(
            publication_action(&expected, &draft, false, &available),
            PublicationAction::Reject(PublicationRejection::StaleCandidate)
        );
    }
}

#[test]
fn draft_cannot_adopt_unknown_or_different_build_evidence() {
    let expected = identity();
    for receipt in [None, Some("another-run-receipt".to_owned())] {
        let draft = PublicationState::Draft {
            source_sha: expected.source_sha.clone(),
            original_receipt_digest: receipt.clone(),
            assets: BTreeMap::new(),
        };
        let reason = if receipt.is_none() {
            PublicationRejection::UnknownPublicationOrigin
        } else {
            PublicationRejection::ConflictingIdentity
        };
        assert_eq!(
            publication_action(
                &expected,
                &draft,
                true,
                &expected.assets.keys().cloned().collect()
            ),
            PublicationAction::Reject(reason)
        );
    }
}

#[test]
fn unexpected_or_changed_assets_cannot_be_overwritten() {
    let expected = identity();
    for assets in [
        BTreeMap::from([("extra".to_owned(), "a".repeat(64))]),
        BTreeMap::from([("payload.tar.gz".to_owned(), "a".repeat(64))]),
    ] {
        let draft = PublicationState::Draft {
            source_sha: expected.source_sha.clone(),
            original_receipt_digest: Some(expected.receipt_digest.clone()),
            assets,
        };
        assert_eq!(
            publication_action(
                &expected,
                &draft,
                true,
                &expected.assets.keys().cloned().collect()
            ),
            PublicationAction::Reject(PublicationRejection::ConflictingIdentity)
        );
    }
}

#[test]
fn missing_original_bytes_block_unfinished_publication() {
    let expected = identity();
    let draft = PublicationState::Draft {
        source_sha: expected.source_sha.clone(),
        original_receipt_digest: Some(expected.receipt_digest.clone()),
        assets: BTreeMap::new(),
    };
    for state in [
        PublicationState::Absent,
        PublicationState::TagOnly {
            source_sha: expected.source_sha.clone(),
            original_receipt_digest: Some(expected.receipt_digest.clone()),
        },
        draft,
    ] {
        assert_eq!(
            publication_action(&expected, &state, true, &BTreeSet::new()),
            PublicationAction::Reject(PublicationRejection::UnavailableOriginalAssets)
        );
    }
}

#[test]
fn completed_immutable_release_only_resumes_distribution() {
    let expected = identity();
    let published = PublicationState::Published {
        identity: expected.clone(),
        immutable: true,
    };
    assert_eq!(
        publication_action(&expected, &published, false, &BTreeSet::new()),
        PublicationAction::ReconcileDistribution
    );
    let mutable = PublicationState::Published {
        identity: expected.clone(),
        immutable: false,
    };
    assert_eq!(
        publication_action(&expected, &mutable, true, &BTreeSet::new()),
        PublicationAction::Reject(PublicationRejection::MutablePublishedRelease)
    );
    let mut changed = expected.clone();
    changed.assets.remove("payload.tar.gz");
    let incomplete = PublicationState::Published {
        identity: changed,
        immutable: true,
    };
    assert_eq!(
        publication_action(&expected, &incomplete, true, &BTreeSet::new()),
        PublicationAction::Reject(PublicationRejection::ConflictingIdentity)
    );
}

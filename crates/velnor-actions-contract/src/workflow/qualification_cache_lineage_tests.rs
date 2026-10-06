use super::*;

#[test]
fn receipt_artifact_name_is_stable() {
    assert_eq!(
        QUALIFICATION_CACHE_RECEIPT_ARTIFACT,
        "velnor-qualification-cache-receipt-v1"
    );
    assert_eq!(
        QUALIFICATION_CACHE_RECEIPT_FILENAME,
        "qualification-cache-receipt.json"
    );
}

#[test]
fn admission_parser_enforces_byte_and_nesting_caps_before_decode() {
    let oversized = vec![b' '; MAX_QUALIFICATION_RECEIPT_BYTES + 1];
    assert!(QualificationCacheAdmission::parse_bounded(&oversized).is_err());

    let too_deep = format!("{}0{}", "[".repeat(33), "]".repeat(33));
    assert!(QualificationCacheAdmission::parse_bounded(too_deep.as_bytes()).is_err());
}

#[test]
fn restore_admission_requires_exact_matched_key_and_backend_id() {
    let expected = QualificationCacheBackendEntry {
        id: 41,
        key: "qualification-k1".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        size_bytes: 4096,
    };
    let keys = BoundQualificationCacheKeys {
        restore: Some(QualificationCacheRestoreExpectation {
            requested_key: expected.key.clone(),
            expected_cache: Some(expected.clone()),
        }),
        save_key: None,
        save_if_state_changes: false,
        expected_prior_state_digest: None,
        runtime_identity_digest: None,
    };
    assert!(
        keys.validate_restore_observation(
            Some(&expected.key),
            &QualificationCacheBackendObservation::Found(expected.clone()),
        )
        .is_ok()
    );
    let prefix = QualificationCacheBackendEntry {
        id: 42,
        key: "qualification-k1-extra".to_owned(),
        ..expected.clone()
    };
    assert!(
        keys.validate_restore_observation(
            Some(&prefix.key),
            &QualificationCacheBackendObservation::Found(prefix.clone()),
        )
        .is_err()
    );
    let wrong_id = QualificationCacheBackendEntry {
        id: 43,
        ..expected.clone()
    };
    assert!(
        keys.validate_restore_observation(
            Some(&expected.key),
            &QualificationCacheBackendObservation::Found(wrong_id),
        )
        .is_err()
    );
}

#[test]
fn cold_restore_requires_observed_miss_and_conditional_saves_skip_unchanged_state() {
    let digest = crate::digest_b3(b"unchanged payload");
    let keys = BoundQualificationCacheKeys {
        restore: Some(QualificationCacheRestoreExpectation {
            requested_key: "qualification-k1".to_owned(),
            expected_cache: None,
        }),
        save_key: Some("qualification-k2".to_owned()),
        save_if_state_changes: true,
        expected_prior_state_digest: Some(digest.clone()),
        runtime_identity_digest: None,
    };
    assert!(
        keys.validate_restore_observation(None, &QualificationCacheBackendObservation::Absent)
            .is_ok()
    );
    assert!(
        keys.validate_restore_observation(
            Some("qualification-k1-prefix"),
            &QualificationCacheBackendObservation::Absent,
        )
        .is_err()
    );
    assert_eq!(
        keys.save_key_for_state(&digest).expect("state digest"),
        None
    );
    assert_eq!(
        keys.save_key_for_state(&crate::digest_b3(b"changed payload"))
            .expect("state digest"),
        Some("qualification-k2")
    );
}

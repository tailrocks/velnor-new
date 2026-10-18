use super::{
    QualificationCacheCensus, parse_slurped_census,
    validate_qualification_cache_backend_observation,
};
use velnor_actions_contract::workflow::{
    BoundQualificationCacheKeys, QualificationCacheBackendEntry,
    QualificationCacheBackendObservation as Observation, QualificationCacheRestoreExpectation,
};

const KEY: &str = "velnor-qualification-tools-abc123";
const REF: &str = "refs/heads/verify-cache";
const VERSION: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn record(id: u64, key: &str, git_ref: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "key": key,
        "ref": git_ref,
        "version": VERSION,
        "last_accessed_at": "2026-10-18T00:00:00.000Z",
        "created_at": "2026-10-17T00:00:00.000Z",
        "size_in_bytes": 17
    })
}

fn page(total_count: u64, entries: Vec<serde_json::Value>) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "total_count": total_count,
        "actions_caches": entries
    }))
    .expect("serialize page")
}

#[test]
fn census_selects_only_exact_key_and_ref() {
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    census
        .push_page(
            1,
            &page(
                3,
                vec![
                    record(1, "velnor-qualification-tools-abc123-prefix", REF),
                    record(2, KEY, "refs/heads/other"),
                    record(3, KEY, REF),
                ],
            ),
        )
        .expect("page");
    assert_eq!(
        census.finish().expect("complete census"),
        Observation::Found(QualificationCacheBackendEntry {
            id: 3,
            key: KEY.to_owned(),
            git_ref: REF.to_owned(),
            version: VERSION.to_owned(),
            size_bytes: 17,
        })
    );
}

#[test]
fn empty_complete_census_is_absent() {
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    census.push_page(1, &page(0, vec![])).expect("empty page");
    assert_eq!(
        census.finish().expect("complete census"),
        Observation::Absent
    );
}

#[test]
fn census_requires_every_page() {
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    let entries = (1..=100).map(|id| record(id, "other-key", REF)).collect();
    census
        .push_page(1, &page(101, entries))
        .expect("first page");
    assert_eq!(census.finish(), Err("cache_census_incomplete"));
}

#[test]
fn census_checks_page_order_and_size() {
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    assert_eq!(
        census.push_page(2, &page(0, vec![])),
        Err("cache_census_page_order_invalid")
    );
    assert_eq!(
        census.push_page(1, &page(1, vec![])),
        Err("cache_census_page_size_invalid")
    );
}

#[test]
fn census_rejects_total_changes_between_pages() {
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    let entries = (1..=100).map(|id| record(id, "other-key", REF)).collect();
    census
        .push_page(1, &page(101, entries))
        .expect("first page");
    assert_eq!(
        census.push_page(2, &page(100, vec![record(101, "other-key", REF)])),
        Err("cache_census_total_changed")
    );
}

#[test]
fn slurped_census_accepts_complete_multiple_pages() {
    let first_page: Vec<_> = (1..=100).map(|id| record(id, "other-key", REF)).collect();
    let response = serde_json::json!([
        { "total_count": 101, "actions_caches": first_page },
        { "total_count": 101, "actions_caches": [record(101, KEY, REF)] }
    ]);
    let bytes = serde_json::to_vec(&response).expect("response");
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    parse_slurped_census(&mut census, &bytes).expect("complete pages");
    assert!(matches!(
        census.finish().expect("complete census"),
        Observation::Found(entry) if entry.id == 101 && entry.key == KEY && entry.git_ref == REF
    ));
}

#[test]
fn census_rejects_duplicate_ids_and_exact_matches() {
    let mut duplicate_id = QualificationCacheCensus::new(KEY, REF).expect("census");
    duplicate_id
        .push_page(
            1,
            &page(2, vec![record(7, "other-key", REF), record(7, KEY, REF)]),
        )
        .expect_err("duplicate ID must fail");

    let mut ambiguous = QualificationCacheCensus::new(KEY, REF).expect("census");
    ambiguous
        .push_page(1, &page(2, vec![record(7, KEY, REF), record(8, KEY, REF)]))
        .expect_err("duplicate exact objects must fail");
}

#[test]
fn census_rejects_invalid_backend_versions() {
    let mut value = record(1, KEY, REF);
    value["version"] = serde_json::json!("not-a-version");
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    assert_eq!(
        census.push_page(1, &page(1, vec![value])),
        Err("cache_census_entry_invalid")
    );
}

#[test]
fn census_rejects_unknown_fields_and_duplicate_json_keys() {
    let mut value = record(1, KEY, REF);
    value["extra"] = serde_json::json!(true);
    let mut unknown = QualificationCacheCensus::new(KEY, REF).expect("census");
    assert_eq!(
        unknown.push_page(1, &page(1, vec![value])),
        Err("cache_census_page_shape_invalid")
    );

    let duplicate = br#"{"total_count":0,"total_count":0,"actions_caches":[]}"#;
    let mut duplicate_key = QualificationCacheCensus::new(KEY, REF).expect("census");
    assert_eq!(
        duplicate_key.push_page(1, duplicate),
        Err("cache_census_page_json_invalid")
    );
}

#[test]
fn census_requires_every_current_github_cache_record_field() {
    let mut value = record(1, KEY, REF);
    value
        .as_object_mut()
        .expect("record object")
        .remove("created_at");
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    assert_eq!(
        census.push_page(1, &page(1, vec![value])),
        Err("cache_census_page_shape_invalid")
    );
}

#[test]
fn census_rejects_oversized_pages_and_invalid_filters() {
    let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
    let oversized = vec![b' '; super::MAX_CACHE_CENSUS_PAGE_BYTES + 1];
    assert_eq!(
        census.push_page(1, &oversized),
        Err("cache_census_page_json_invalid")
    );
    assert!(QualificationCacheCensus::new("", REF).is_err());
    assert!(QualificationCacheCensus::new(KEY, "refs/heads/has space").is_err());
}

#[test]
fn census_rejects_overlong_keys_refs_and_returned_record_identity() {
    assert!(
        QualificationCacheCensus::new(
            &"k".repeat(velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES + 1),
            REF,
        )
        .is_err()
    );
    assert!(
        QualificationCacheCensus::new(KEY, &"r".repeat(super::MAX_CACHE_REF_BYTES + 1),).is_err()
    );

    for invalid in [
        record(
            1,
            &"k".repeat(velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES + 1),
            REF,
        ),
        record(1, KEY, "refs/heads/has space"),
    ] {
        let mut census = QualificationCacheCensus::new(KEY, REF).expect("census");
        assert_eq!(
            census.push_page(1, &page(1, vec![invalid])),
            Err("cache_census_entry_invalid")
        );
    }
}

fn expected_entry() -> QualificationCacheBackendEntry {
    QualificationCacheBackendEntry {
        id: 91,
        key: KEY.to_owned(),
        git_ref: REF.to_owned(),
        version: VERSION.to_owned(),
        size_bytes: 17,
    }
}

fn bound_keys(
    expected_cache: Option<QualificationCacheBackendEntry>,
) -> BoundQualificationCacheKeys {
    BoundQualificationCacheKeys {
        restore: Some(QualificationCacheRestoreExpectation {
            requested_key: KEY.to_owned(),
            expected_cache,
        }),
        save_key: None,
        save_if_state_changes: false,
        expected_prior_state_digest: None,
        runtime_identity_digest: None,
    }
}

#[test]
fn backend_admission_requires_action_hit_key_and_exact_census_agreement() {
    let expected = expected_entry();
    let keys = bound_keys(Some(expected.clone()));
    let observed = Observation::Found(expected.clone());
    assert!(
        validate_qualification_cache_backend_observation(&keys, true, Some(KEY), &observed).is_ok()
    );

    assert!(
        validate_qualification_cache_backend_observation(&keys, false, Some(KEY), &observed)
            .is_err()
    );
    assert!(
        validate_qualification_cache_backend_observation(
            &keys,
            true,
            Some("prefix-key"),
            &observed
        )
        .is_err()
    );
    assert!(
        validate_qualification_cache_backend_observation(
            &keys,
            true,
            Some(KEY),
            &Observation::Absent
        )
        .is_err()
    );

    let mut wrong_id = expected.clone();
    wrong_id.id += 1;
    let mut wrong_ref = expected.clone();
    wrong_ref.git_ref.push_str("/other");
    let mut wrong_version = expected.clone();
    wrong_version.version.replace_range(0..1, "f");
    let mut wrong_size = expected;
    wrong_size.size_bytes += 1;
    for wrong in [wrong_id, wrong_ref, wrong_version, wrong_size] {
        assert!(
            validate_qualification_cache_backend_observation(
                &keys,
                true,
                Some(KEY),
                &Observation::Found(wrong)
            )
            .is_err()
        );
    }
}

#[test]
fn backend_admission_accepts_only_a_required_cold_miss_or_disabled_layer() {
    let cold = bound_keys(None);
    assert!(
        validate_qualification_cache_backend_observation(&cold, false, None, &Observation::Absent)
            .is_ok()
    );
    assert!(
        validate_qualification_cache_backend_observation(
            &cold,
            false,
            None,
            &Observation::Found(expected_entry())
        )
        .is_err()
    );
    assert!(
        validate_qualification_cache_backend_observation(
            &cold,
            true,
            Some(KEY),
            &Observation::Absent
        )
        .is_err()
    );

    let disabled = BoundQualificationCacheKeys {
        restore: None,
        save_key: None,
        save_if_state_changes: false,
        expected_prior_state_digest: None,
        runtime_identity_digest: None,
    };
    assert!(
        validate_qualification_cache_backend_observation(
            &disabled,
            false,
            None,
            &Observation::NotQueried
        )
        .is_ok()
    );
    assert!(
        validate_qualification_cache_backend_observation(
            &disabled,
            false,
            None,
            &Observation::Absent
        )
        .is_err()
    );
}

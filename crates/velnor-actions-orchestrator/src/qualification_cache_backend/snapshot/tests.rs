use super::census::{cache_ref_api_request, parse_ref_pages};
use super::*;
use velnor_actions_contract::workflow::{
    QualificationDispatchMode, QualificationRunRef, verification_repeat_campaign,
};
use velnor_actions_mise::PinnedTool;

const SOURCE: &str = "0123456789abcdef0123456789abcdef01234567";
const REF: &str = "refs/heads/main";
const VERSION: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn context(phase: QualificationPhase) -> QualificationDispatch {
    QualificationDispatch {
        mode: QualificationDispatchMode::Controlled,
        campaign: "snapshot-test".to_owned(),
        phase,
        repository: "owner/project".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: REF.to_owned(),
        ref_protected: true,
        workflow_ref: "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: SOURCE.to_owned(),
        source_sha: SOURCE.to_owned(),
        run_id: 801,
        run_attempt: 1,
        predecessor: match phase {
            QualificationPhase::Warm => Some(QualificationRunRef {
                run_id: 800,
                run_attempt: 1,
            }),
            QualificationPhase::Third => Some(QualificationRunRef {
                run_id: 800,
                run_attempt: 2,
            }),
            QualificationPhase::UsefulDelta => Some(QualificationRunRef {
                run_id: 800,
                run_attempt: 3,
            }),
            _ => None,
        },
        scale_set_image_source_sha: None,
    }
}

fn repeat_context(attempt: u32) -> QualificationDispatch {
    let workflow_ref = format!("owner/project/.github/workflows/qualification.yml@{REF}");
    let campaign = verification_repeat_campaign(
        "owner/project",
        "main",
        REF,
        &workflow_ref,
        SOURCE,
        SOURCE,
        802,
    );
    QualificationDispatch {
        mode: QualificationDispatchMode::VerificationOnly,
        campaign,
        phase: if attempt == 1 {
            QualificationPhase::Cold
        } else {
            QualificationPhase::Warm
        },
        repository: "owner/project".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: REF.to_owned(),
        ref_protected: true,
        workflow_ref,
        workflow_sha: SOURCE.to_owned(),
        source_sha: SOURCE.to_owned(),
        run_id: 802,
        run_attempt: attempt,
        predecessor: (attempt > 1).then_some(QualificationRunRef {
            run_id: 802,
            run_attempt: 1,
        }),
        scale_set_image_source_sha: None,
    }
}

fn entry(id: u64, key: &str) -> QualificationCacheBackendEntry {
    QualificationCacheBackendEntry {
        id,
        key: key.to_owned(),
        git_ref: REF.to_owned(),
        version: VERSION.to_owned(),
        size_bytes: 12,
    }
}

fn api_entry(id: u64, key: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "key": key,
        "ref": REF,
        "version": VERSION,
        "created_at": "2026-10-20T00:00:00Z",
        "last_accessed_at": null,
        "size_in_bytes": 12
    })
}

#[test]
fn control_capture_is_explicitly_not_queried_and_bound_to_attempt() {
    let dispatch = context(QualificationPhase::Control);
    let snapshot = capture_qualification_cache_before(Path::new("."), &dispatch)
        .expect("control capture needs no API");
    assert_eq!(snapshot.status, "not_queried");
    assert_eq!(
        snapshot.observation_for("key", REF).expect("observation"),
        QualificationCacheBackendObservation::NotQueried
    );
    let bytes = canonical_json_bytes(&snapshot).expect("canonical snapshot");
    assert_eq!(
        parse_qualification_cache_before(&bytes, &dispatch).expect("parse"),
        snapshot
    );
    let mut wrong_attempt = dispatch;
    wrong_attempt.run_attempt += 1;
    assert!(parse_qualification_cache_before(&bytes, &wrong_attempt).is_err());
}

#[test]
fn verification_only_cold_and_warm_contexts_bind_exact_attempts() {
    for attempt in [1, 2] {
        let dispatch = repeat_context(attempt);
        dispatch.validate_shape().expect("valid repeat context");
        let snapshot = complete_snapshot_for_test(&dispatch).expect("snapshot");
        let bytes = canonical_json_bytes(&snapshot).expect("canonical snapshot");
        assert_eq!(
            parse_qualification_cache_before(&bytes, &dispatch).expect("parse"),
            snapshot
        );
        let mut other = dispatch.clone();
        other.run_attempt += 1;
        assert!(parse_qualification_cache_before(&bytes, &other).is_err());
    }
}

#[test]
fn complete_snapshot_selects_exact_key_and_ref_and_rejects_ambiguity() {
    let dispatch = context(QualificationPhase::Cold);
    let snapshot = QualificationCacheBackendSnapshot::new(
        &dispatch,
        run_key_for_ci(dispatch.run_id, u64::from(dispatch.run_attempt)),
        "complete",
        Some(10),
        Some(11),
        vec![entry(1, "other"), entry(2, "wanted")],
    )
    .expect("snapshot");
    assert_eq!(
        snapshot.observation_for("wanted", REF).expect("found"),
        QualificationCacheBackendObservation::Found(entry(2, "wanted"))
    );
    assert_eq!(
        snapshot
            .observation_for("missing", REF)
            .expect("complete absence"),
        QualificationCacheBackendObservation::Absent
    );
    assert!(
        snapshot
            .observation_for("wanted", "refs/heads/other")
            .is_err()
    );
    let ambiguous = QualificationCacheBackendSnapshot::new(
        &dispatch,
        run_key_for_ci(dispatch.run_id, u64::from(dispatch.run_attempt)),
        "complete",
        Some(10),
        Some(11),
        vec![entry(2, "wanted"), entry(3, "wanted")],
    )
    .expect("snapshot with distinct backend versions");
    assert!(ambiguous.observation_for("wanted", REF).is_err());
}

#[test]
fn snapshot_digest_and_canonical_encoding_are_required() {
    let dispatch = context(QualificationPhase::Cold);
    let snapshot = complete_snapshot_for_test(&dispatch).expect("snapshot");
    let mut value = serde_json::to_value(&snapshot).expect("value");
    value["entries"] = serde_json::json!([entry(2, "x")]);
    let tampered = canonical_json_bytes(&value).expect("tampered canonical");
    assert!(parse_qualification_cache_before(&tampered, &dispatch).is_err());
    let bytes = canonical_json_bytes(&snapshot).expect("canonical");
    let pretty = serde_json::to_vec_pretty(&snapshot).expect("pretty");
    assert_ne!(bytes, pretty);
    assert!(parse_qualification_cache_before(&pretty, &dispatch).is_err());
    let unknown = serde_json::json!({"unexpected":true});
    let mut value = serde_json::to_value(&snapshot).expect("value");
    value["unexpected"] = unknown["unexpected"].clone();
    let unknown = canonical_json_bytes(&value).expect("unknown field");
    assert!(parse_qualification_cache_before(&unknown, &dispatch).is_err());
    let duplicate = String::from_utf8(bytes)
        .expect("utf-8")
        .replacen('{', "{\"schema\":1,", 1);
    assert!(parse_qualification_cache_before(duplicate.as_bytes(), &dispatch).is_err());
}

#[test]
fn before_and_after_snapshots_are_stage_bound() {
    let dispatch = context(QualificationPhase::Cold);
    let before = snapshot_for_test(&dispatch, "before", vec![entry(1, "cache-key")])
        .expect("before snapshot");
    let after =
        snapshot_for_test(&dispatch, "after", vec![entry(2, "cache-key")]).expect("after snapshot");
    let before_bytes = canonical_json_bytes(&before).expect("before bytes");
    let after_bytes = canonical_json_bytes(&after).expect("after bytes");

    assert_eq!(
        parse_qualification_cache_before(&before_bytes, &dispatch).expect("parse before"),
        before
    );
    assert_eq!(
        parse_qualification_cache_after(&after_bytes, &dispatch).expect("parse after"),
        after
    );
    assert!(parse_qualification_cache_before(&after_bytes, &dispatch).is_err());
    assert!(parse_qualification_cache_after(&before_bytes, &dispatch).is_err());
}

#[test]
fn not_queried_is_for_control_only() {
    let cold = context(QualificationPhase::Cold);
    assert!(
        QualificationCacheBackendSnapshot::new(
            &cold,
            run_key_for_ci(cold.run_id, u64::from(cold.run_attempt)),
            "not_queried",
            None,
            None,
            vec![],
        )
        .is_err()
    );
}

#[test]
fn ref_census_requires_all_pages_unique_ids_and_exact_ref() {
    let page = |total, caches: Vec<serde_json::Value>| serde_json::json!({"total_count": total, "actions_caches": caches});
    let complete = serde_json::to_vec(&serde_json::json!([page(
        2,
        vec![api_entry(1, "a"), api_entry(2, "b")]
    )]))
    .expect("response");
    assert_eq!(parse_ref_pages(&complete, REF).expect("census").len(), 2);
    let duplicate = serde_json::to_vec(&serde_json::json!([page(
        2,
        vec![api_entry(1, "a"), api_entry(1, "b")]
    )]))
    .expect("response");
    assert!(parse_ref_pages(&duplicate, REF).is_err());
    let wrong_ref = serde_json::to_vec(&serde_json::json!([
            page(1, vec![serde_json::json!({"id":1,"key":"a","ref":"refs/heads/other","version":VERSION,"created_at":"x","last_accessed_at":null,"size_in_bytes":1})])
        ])).expect("response");
    assert!(parse_ref_pages(&wrong_ref, REF).is_err());
    let incomplete = serde_json::to_vec(&serde_json::json!([page(1, vec![])])).expect("response");
    assert!(parse_ref_pages(&incomplete, REF).is_err());
    let first: Vec<_> = (1..=100).map(|id| api_entry(id, "key")).collect();
    let second = api_entry(101, "key");
    let two_pages = serde_json::to_vec(&serde_json::json!([
        page(101, first),
        page(101, vec![second]),
    ]))
    .expect("two pages");
    assert_eq!(
        parse_ref_pages(&two_pages, REF).expect("all pages").len(),
        101
    );
    let changed_total = serde_json::to_vec(&serde_json::json!([
        page(101, (1..=100).map(|id| api_entry(id, "key")).collect()),
        page(100, vec![api_entry(101, "key")]),
    ]))
    .expect("changed total");
    assert!(parse_ref_pages(&changed_total, REF).is_err());
}

#[test]
fn ref_census_request_uses_pinned_gh_without_runtime_cache_key() {
    let request = cache_ref_api_request("owner/project", REF).expect("request");
    assert_eq!(request.tools(), &[PinnedTool::Gh]);
    assert_eq!(request.program(), "gh");
    let args: Vec<_> = request
        .args()
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "api",
            "--method",
            "GET",
            "repos/owner/project/actions/caches",
            "--paginate",
            "--slurp",
            "-f",
            "ref=refs/heads/main",
            "-F",
            "per_page=100",
        ]
    );
}

use super::*;

const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BRANCH: &str = "main";
const RUN_ID: u64 = 29;

fn compatibility() -> String {
    velnor_actions_contract::digest_b3(b"baseline compatibility")
}

fn artifact_name() -> String {
    velnor_actions_contract::artifact_id_for_baseline(BASE, &compatibility())
        .expect("artifact name")
}

fn artifact(name: &str, id: u64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "expired": false,
        "size_in_bytes": 128,
        "digest": format!("sha256:{}", "b".repeat(64)),
        "workflow_run": {
            "id": RUN_ID,
            "head_sha": BASE,
            "head_branch": BRANCH,
        },
    })
}

fn listing(entries: Vec<serde_json::Value>) -> String {
    serde_json::json!([{
        "total_count": entries.len(),
        "artifacts": entries,
    }])
    .to_string()
}

#[test]
fn exact_name_returns_service_receipt_without_run_attempt_suffix() {
    let name = artifact_name();
    assert_eq!(name, format!("velnor-baseline-{BASE}-{}", compatibility()));
    let listed = listing(vec![artifact(&name, 73)]);
    let receipt = select_baseline_artifacts(&listed, &name, BASE, &compatibility(), BRANCH, RUN_ID)
        .expect("selected receipt")
        .pop()
        .expect("one receipt");
    assert_eq!(receipt.service_id, 73);
    assert_eq!(receipt.sha256, "b".repeat(64));
    assert_eq!(receipt.size_bytes, 128);
    assert_eq!(receipt.compatibility_id, compatibility());
    assert_ne!(
        receipt.service_id,
        crate::cover_compat::baseline_artifact_numeric_id(&receipt.name)
    );
    assert_eq!(receipt.run_id, RUN_ID);
    assert_eq!(receipt.head_sha, BASE);
    assert_eq!(receipt.head_branch, BRANCH);
}

#[test]
fn exact_name_receipts_must_be_digest_attested() {
    let name = artifact_name();
    let duplicate = listing(vec![artifact(&name, 71), artifact(&name, 72)]);
    let receipts =
        select_baseline_artifacts(&duplicate, &name, BASE, &compatibility(), BRANCH, RUN_ID)
            .expect("same stable name can identify multiple attempts");
    assert_eq!(receipts.len(), 2);
    assert_ne!(receipts[0].service_id, receipts[1].service_id);

    let mut missing_digest = artifact(&name, 71);
    missing_digest["digest"] = serde_json::Value::Null;
    assert_eq!(
        select_baseline_artifacts(
            &listing(vec![missing_digest]),
            &name,
            BASE,
            &compatibility(),
            BRANCH,
            RUN_ID,
        )
        .expect_err("missing service digest"),
        "baseline_artifact_digest_missing"
    );
}

#[test]
fn expired_exact_name_sibling_does_not_poison_live_receipt() {
    let name = artifact_name();
    let mut expired = artifact(&name, 71);
    expired["expired"] = true.into();
    let listed = listing(vec![expired, artifact(&name, 72)]);
    let receipts =
        select_baseline_artifacts(&listed, &name, BASE, &compatibility(), BRANCH, RUN_ID)
            .expect("expired sibling is not a live candidate");
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].service_id, 72);
}

#[test]
fn exact_name_requires_complete_scope_and_bounded_regular_receipt() {
    let name = artifact_name();
    for field in ["expired", "id", "head", "branch", "run", "size", "digest"] {
        let mut entry = artifact(&name, 71);
        match field {
            "expired" => entry["expired"] = serde_json::Value::Null,
            "id" => entry["id"] = 0.into(),
            "head" => entry["workflow_run"]["head_sha"] = "c".repeat(40).into(),
            "branch" => entry["workflow_run"]["head_branch"] = "topic".into(),
            "run" => entry["workflow_run"]["id"] = 30.into(),
            "size" => entry["size_in_bytes"] = (9 * 1024 * 1024).into(),
            "digest" => entry["digest"] = "sha256:not-a-digest".into(),
            _ => unreachable!("fixture field"),
        }
        assert!(
            select_baseline_artifacts(
                &listing(vec![entry]),
                &name,
                BASE,
                &compatibility(),
                BRANCH,
                RUN_ID,
            )
            .is_err(),
            "{field} must fail closed"
        );
    }
}

#[test]
fn listing_must_be_fully_paginated_and_unambiguous_json() {
    let name = artifact_name();
    let first = serde_json::json!({
        "total_count": 2,
        "artifacts": [artifact(&name, 71)],
    });
    let second = serde_json::json!({
        "total_count": 2,
        "artifacts": [artifact("other", 72)],
    });
    assert!(
        select_baseline_artifacts(
            &serde_json::json!([first, second]).to_string(),
            &name,
            BASE,
            &compatibility(),
            BRANCH,
            RUN_ID,
        )
        .is_ok()
    );

    let incomplete = serde_json::json!([{
        "total_count": 2,
        "artifacts": [artifact(&name, 71)],
    }]);
    assert_eq!(
        select_baseline_artifacts(
            &incomplete.to_string(),
            &name,
            BASE,
            &compatibility(),
            BRANCH,
            RUN_ID,
        )
        .expect_err("truncated listing"),
        "baseline_listing_incomplete"
    );

    let inconsistent_pages = serde_json::json!([
        {"total_count": 2, "artifacts": [artifact(&name, 71)]},
        {"total_count": 3, "artifacts": [artifact("other", 72)]}
    ]);
    assert_eq!(
        select_baseline_artifacts(
            &inconsistent_pages.to_string(),
            &name,
            BASE,
            &compatibility(),
            BRANCH,
            RUN_ID,
        )
        .expect_err("inconsistent page totals"),
        "baseline_listing_incomplete"
    );

    let beyond_bound = serde_json::json!([{
        "total_count": 10_001,
        "artifacts": [],
    }]);
    assert_eq!(
        select_baseline_artifacts(
            &beyond_bound.to_string(),
            &name,
            BASE,
            &compatibility(),
            BRANCH,
            RUN_ID,
        )
        .expect_err("listing count exceeds the accepted bound"),
        "baseline_listing_oversize"
    );

    let duplicate_key = listing(vec![artifact(&name, 71)])
        .replace("\"expired\":false", "\"expired\":true,\"expired\":false");
    assert!(
        select_baseline_artifacts(
            &duplicate_key,
            &name,
            BASE,
            &compatibility(),
            BRANCH,
            RUN_ID,
        )
        .is_err()
    );
}

#[test]
fn listing_rejects_overlapping_or_invalid_ids_before_filtering_entries() {
    let name = artifact_name();
    let mut unrelated_expired = artifact("other", 72);
    unrelated_expired["expired"] = true.into();
    let overlapping_pages = serde_json::json!([
        {"total_count": 2, "artifacts": [artifact("other", 72)]},
        {"total_count": 2, "artifacts": [unrelated_expired]}
    ]);
    assert_eq!(
        select_baseline_artifacts(
            &overlapping_pages.to_string(),
            &name,
            BASE,
            &compatibility(),
            BRANCH,
            RUN_ID,
        )
        .expect_err("same service ID on multiple pages"),
        "baseline_listing_overlap"
    );

    let mut idless_unrelated = artifact("other", 72);
    idless_unrelated["id"] = serde_json::Value::Null;
    assert_eq!(
        select_baseline_artifacts(
            &listing(vec![idless_unrelated]),
            &name,
            BASE,
            &compatibility(),
            BRANCH,
            RUN_ID,
        )
        .expect_err("every service entry needs a positive ID"),
        "baseline_listing_invalid"
    );
}

#[test]
fn attempt_suffixed_names_are_not_baseline_artifacts() {
    let suffixed = format!("{}-r{RUN_ID}-a2", artifact_name());
    assert!(
        select_named_baseline_artifacts(
            &listing(vec![artifact(&suffixed, 73)]),
            &suffixed,
            BASE,
            BRANCH,
            RUN_ID,
        )
        .is_err()
    );
}

#[test]
fn selected_run_requires_exact_success_and_attempt() {
    let success = serde_json::json!([{
        "databaseId": RUN_ID,
        "headSha": BASE,
        "headBranch": BRANCH,
        "event": "push",
        "conclusion": "success",
        "attempt": 2,
    }]);
    assert_eq!(
        select_exact_base_run(&success.to_string(), BASE, BRANCH),
        Ok(SelectedBaseRun {
            run_id: RUN_ID,
            attempt: 2,
        })
    );

    let failed = serde_json::json!([{
        "databaseId": RUN_ID,
        "headSha": BASE,
        "headBranch": BRANCH,
        "event": "push",
        "conclusion": "failure",
        "attempt": 2,
    }]);
    assert!(select_exact_base_run(&failed.to_string(), BASE, BRANCH).is_err());
}

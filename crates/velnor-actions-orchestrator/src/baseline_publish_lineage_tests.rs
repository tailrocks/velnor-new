//! Publisher lineage fields: direct proofs start a fresh chain.

use super::*;
use crate::cover_baseline::provenance_check::validate_manifest_lineage;

#[test]
fn publish_stages_trusted_manifest_under_derived_name() {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let temp = staged_run(&plan, "r7-a1");
    let outputs = baseline_publish_to(&request_json(&head), "r7-a1", temp.path()).expect("publish");
    let compat = crate::cover_compat::baseline_compat_for_plan(&plan).expect("compat");
    let expected = artifact_id_for_baseline(&head, &compat).expect("name");
    assert_eq!(outputs.artifact_name, expected);
    let staged = staged_manifest(temp.path(), "r7-a1");
    assert_eq!(staged["schema"], 2);
    assert_eq!(staged["source_commit"], head);
    assert_eq!(staged["ref"], "refs/heads/testmain");
    assert_eq!(staged["event"], "push");
    assert_eq!(
        staged["workflow_ref"],
        "o/r/.github/workflows/ci.yml@refs/heads/testmain"
    );
    assert_eq!(staged["run_id"], 7);
    assert_eq!(staged["run_attempt"], 1);
    assert_eq!(staged["final_status"], "passed");
    assert_eq!(staged["compatibility_id"], compat);
    assert_eq!(staged["artifact_name"], expected);
    assert_eq!(
        staged["artifact_id"],
        crate::cover_compat::baseline_artifact_numeric_id(&expected)
    );
    assert_eq!(staged["parent"], serde_json::Value::Null);
    let tasks = staged["tasks"].as_array().expect("tasks");
    assert_eq!(tasks.len(), 2);
    assert!(tasks.iter().all(|task| task["carried_from"].is_null()));
}

/// First direct publication, retained as the immediate source manifest.
fn direct_parent() -> BaselineManifest {
    let head = "a".repeat(40);
    let plan = fixture_plan(&head, "r7-a1");
    let temp = staged_run(&plan, "r7-a1");
    baseline_publish_to(
        &request_json_with_base(&head, &"b".repeat(40)),
        "r7-a1",
        temp.path(),
    )
    .expect("direct publish");
    serde_json::from_value(staged_manifest(temp.path(), "r7-a1")).expect("parent manifest")
}

/// A current plan covering its test obligation with a specific parent.
fn covered_plan(parent: &BaselineManifest, head: &str, run_key: &str) -> Plan {
    let mut plan = fixture_plan(head, run_key);
    plan.base = Some(parent.source_commit.clone());
    let source = parent
        .tasks
        .iter()
        .find(|task| task.task_id == plan.obligations[1].task_id)
        .expect("parent task");
    let digest = digest_b3(&canonical_json_bytes(parent).expect("parent bytes"));
    plan.obligations[1].decision = ObligationDecision::CoveredByTrustedBaseline;
    plan.obligations[1].baseline_proof = Some(
        velnor_actions_contract::BaselineProof::new(
            &parent.source_commit,
            source.proof_run_id,
            parent.artifact_id,
            &parent.artifact_name,
            &digest,
        )
        .expect("task proof"),
    );
    let covered_task_id = plan.obligations[1].task_id.clone();
    plan.matrix
        .include
        .retain(|entry| entry.task_id != covered_task_id);
    plan.baseline = PlanBaseline::used(
        &parent.source_commit,
        parent.run_id,
        parent.artifact_id,
        &parent.artifact_name,
        &digest,
    )
    .expect("plan baseline");
    plan.validate().expect("covered plan validates");
    plan
}

/// Publish one covered plan with its exact parent staged as plan evidence.
fn publish_carry(parent: &BaselineManifest, head: &str, run_key: &str) -> BaselineManifest {
    let plan = covered_plan(parent, head, run_key);
    let temp = staged_run(&plan, run_key);
    let parent_bytes = canonical_json_bytes(parent).expect("parent bytes");
    std::fs::write(
        temp.path()
            .join("velnor")
            .join(run_key)
            .join(BASELINE_FILENAME),
        &parent_bytes,
    )
    .expect("stage parent");
    let request = request_json_with_base(head, &parent.source_commit);
    baseline_publish_to(&request, run_key, temp.path()).expect("carry publish");
    assert_eq!(
        std::fs::read(
            temp.path()
                .join("velnor")
                .join(run_key)
                .join(BASELINE_FILENAME),
        )
        .expect("parent retained"),
        parent_bytes
    );
    serde_json::from_value(staged_manifest(temp.path(), run_key)).expect("carried manifest")
}

#[test]
fn publisher_carries_only_exact_covered_task_and_preserves_origin_run() {
    let parent = direct_parent();
    let parent_bytes = canonical_json_bytes(&parent).expect("parent bytes");
    let parent_digest = digest_b3(&parent_bytes);
    let next = publish_carry(&parent, &"c".repeat(40), "r8-a1");
    let carried = next
        .tasks
        .iter()
        .find(|task| task.task_id == "stack/rust/demo/test/default")
        .expect("carried test");
    assert_eq!(carried.proof_run_id, parent.run_id);
    assert_eq!(carried.observed_run_id, next.run_id);
    assert_eq!(
        carried
            .carried_from
            .as_ref()
            .expect("parent binding")
            .run_id(),
        parent.run_id
    );
    assert_eq!(
        carried
            .carried_from
            .as_ref()
            .expect("parent binding")
            .manifest_digest(),
        parent_digest
    );
    assert_eq!(
        next.parent.as_deref().map(|entry| entry.run_id),
        Some(parent.run_id)
    );
    let executed = next
        .tasks
        .iter()
        .find(|task| task.task_id == "stack/rust/demo/clippy/default")
        .expect("direct task");
    assert_eq!(executed.proof_run_id, next.run_id);
    assert!(executed.carried_from.is_none());
}

#[test]
fn repeated_carries_keep_original_execution_and_record_each_observer() {
    let first = direct_parent();
    let second = publish_carry(&first, &"c".repeat(40), "r8-a1");
    let third = publish_carry(&second, &"d".repeat(40), "r9-a1");
    let carried = third
        .tasks
        .iter()
        .find(|task| task.task_id == "stack/rust/demo/test/default")
        .expect("third carried task");
    assert_eq!(carried.proof_run_id, first.run_id);
    assert_eq!(carried.observed_run_id, third.run_id);
    assert_eq!(
        carried.carried_from.as_ref().expect("binding").run_id(),
        second.run_id
    );
    assert_eq!(
        third.parent.as_deref().map(|entry| entry.run_id),
        Some(second.run_id)
    );
    assert_eq!(
        third
            .parent
            .as_deref()
            .and_then(|parent| parent.parent.as_deref())
            .map(|parent| parent.run_id),
        Some(first.run_id)
    );
    assert!(validate_manifest_lineage(&third).is_ok());
}

/// Make one valid carry node for the publisher-boundary cap fixture.
fn synthetic_carry(parent: BaselineManifest, source_commit: &str, run_id: u64) -> BaselineManifest {
    let digest = digest_b3(&canonical_json_bytes(&parent).expect("parent bytes"));
    let name = crate::cover_baseline::provenance_check::baseline_artifact_name(
        source_commit,
        &parent.compatibility_id,
    )
    .expect("artifact name");
    let mut child = parent.clone();
    child.source_commit = source_commit.to_owned();
    child.artifact_name = name.clone();
    child.artifact_id = crate::cover_compat::baseline_artifact_numeric_id(&name);
    child.run_id = run_id;
    child.run_attempt = 1;
    for task in &mut child.tasks {
        task.observed_run_id = run_id;
        task.carried_from = Some(
            velnor_actions_contract::BaselineProof::new(
                &parent.source_commit,
                parent.run_id,
                parent.artifact_id,
                &parent.artifact_name,
                &digest,
            )
            .expect("parent binding"),
        );
    }
    child.parent = Some(Box::new(parent));
    child
}

#[test]
fn planner_preflight_refuses_a_carry_that_would_exceed_depth_cap() {
    let mut parent = direct_parent();
    for run_id in 8..39 {
        parent = synthetic_carry(parent, &format!("{run_id:040x}"), run_id);
    }
    assert!(validate_manifest_lineage(&parent).is_ok());
    let plan = covered_plan(&parent, &"d".repeat(40), "r39-a1");
    assert!(!carry_candidate_fits(
        &plan,
        &parent,
        "testmain",
        Some("o/r")
    ));
}

#[test]
fn planner_preflight_refuses_a_carry_that_would_exceed_byte_cap() {
    let mut parent = direct_parent();
    parent.tasks[0].external_data = Some(crate::external_data::ExternalDataFreshness {
        source: String::new(),
        identity: digest_b3(b"external data"),
        age_secs: 0,
    });
    let base_size = canonical_json_bytes(&parent).expect("base parent").len();
    let source_len = crate::merge::required_evidence::MAX_BASELINE_MANIFEST_BYTES - base_size - 1;
    parent.tasks[0]
        .external_data
        .as_mut()
        .expect("freshness")
        .source = "x".repeat(source_len);
    assert_eq!(
        canonical_json_bytes(&parent)
            .expect("near-limit parent")
            .len(),
        crate::merge::required_evidence::MAX_BASELINE_MANIFEST_BYTES - 1
    );
    let plan = covered_plan(&parent, &"d".repeat(40), "r8-a1");
    assert!(!carry_candidate_fits(
        &plan,
        &parent,
        "testmain",
        Some("o/r")
    ));
}

#[test]
fn planner_preflight_refuses_to_reuse_unbounded_external_freshness() {
    let mut parent = direct_parent();
    parent.tasks[1].external_data = Some(crate::external_data::ExternalDataFreshness {
        source: "advisory-db".to_owned(),
        identity: digest_b3(b"advisory snapshot"),
        age_secs: 0,
    });
    let plan = covered_plan(&parent, &"d".repeat(40), "r8-a1");
    assert!(!carry_candidate_fits(
        &plan,
        &parent,
        "testmain",
        Some("o/r")
    ));
}

#[test]
fn publisher_refuses_parent_that_disagrees_with_plan_proof() {
    let parent = direct_parent();
    let head = "c".repeat(40);
    let mut plan = covered_plan(&parent, &head, "r8-a1");
    plan.obligations[1].input_digest = digest(31);
    plan.validate().expect("well-shaped forged plan");
    let temp = staged_run(&plan, "r8-a1");
    std::fs::write(
        temp.path()
            .join("velnor")
            .join("r8-a1")
            .join(BASELINE_FILENAME),
        canonical_json_bytes(&parent).expect("parent bytes"),
    )
    .expect("stage parent");
    let error = baseline_publish_to(
        &request_json_with_base(&head, &parent.source_commit),
        "r8-a1",
        temp.path(),
    )
    .expect_err("mismatched input must refuse");
    assert!(error.to_string().contains("parent_task_mismatch"));
}

//! Protected carry ancestry regressions.

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for, verdict};
use velnor_actions_contract::BaselineProof;

fn carry(parent: BaselineManifest, commit: &str, run_id: u64) -> BaselineManifest {
    let digest = digest_b3(&canonical_json_bytes(&parent).expect("canonical parent"));
    let mut child = parent.clone();
    child.source_commit = commit.to_owned();
    child.artifact_name = baseline_artifact_name(commit, &child.compatibility_id).expect("name");
    child.artifact_id = crate::cover_compat::baseline_artifact_numeric_id(&child.artifact_name);
    child.run_id = run_id;
    child.run_attempt = 1;
    for task in &mut child.tasks {
        task.observed_run_id = run_id;
        task.carried_from = Some(
            BaselineProof::new(
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
fn carried_origin_survives_multiple_protected_runs_and_merge() {
    let first = "a".repeat(40);
    let second = "b".repeat(40);
    let third = "c".repeat(40);
    let mut origin = manifest_for(&first);
    origin.repository_id = digest_b3(b"github.com/o/r");
    let carried = carry(carry(origin, &second, 8), &third, 9);
    assert!(validate_manifest_lineage(&carried).is_ok());
    assert_eq!(carried.tasks[0].proof_run_id, 7);
    assert_eq!(carried.tasks[0].observed_run_id, 9);
    let expected = super::super::ProvenanceExpectations {
        base: carried.source_commit.clone(),
        branch: "testmain".to_owned(),
        workflow_path: ".github/workflows/ci.yml".to_owned(),
        generator_version: carried.generator_version.clone(),
        generator_sha256: carried.generator_sha256.clone(),
        repository_id: Some(carried.repository_id.clone()),
        repository_slug: Some("o/r".to_owned()),
        repository_conflict: false,
    };
    let digest = digest_b3(&canonical_json_bytes(&carried).expect("canonical manifest"));
    assert!(super::super::validate_provenance(&carried, &digest, &expected).is_ok());
    let plan = plan_for(&carried, Some(&carried.source_commit));
    assert!(!verdict(&plan, Some(&carried)).0.planning_failed);
}

#[test]
fn arbitrary_origin_without_parent_fails() {
    let mut manifest = manifest_for(&"a".repeat(40));
    manifest.tasks[0].proof_run_id = 5;
    assert_eq!(
        validate_manifest_lineage(&manifest),
        Err("originating_run_unverified".to_owned())
    );
}

#[test]
fn changed_parent_or_task_identity_fails() {
    let original = manifest_for(&"a".repeat(40));
    let mut carried = carry(original, &"b".repeat(40), 8);
    carried.parent.as_mut().expect("parent").run_attempt = 2;
    assert!(validate_manifest_lineage(&carried).is_err());

    let original = manifest_for(&"a".repeat(40));
    let mut carried = carry(original, &"b".repeat(40), 8);
    carried.tasks[0].input_digest = digest_b3(b"changed input");
    assert!(validate_manifest_lineage(&carried).is_err());

    let original = manifest_for(&"a".repeat(40));
    let mut carried = carry(original, &"b".repeat(40), 8);
    carried.tasks[0].task_digest = digest_b3(b"changed task");
    assert!(validate_manifest_lineage(&carried).is_err());

    let original = manifest_for(&"a".repeat(40));
    let mut carried = carry(original, &"b".repeat(40), 8);
    carried.tasks[0].closure_digest = digest_b3(b"changed closure");
    assert!(validate_manifest_lineage(&carried).is_err());

    let original = manifest_for(&"a".repeat(40));
    let mut carried = carry(original, &"b".repeat(40), 8);
    carried.tasks[0].observed_run_id = 7;
    assert!(validate_manifest_lineage(&carried).is_err());
}

#[test]
fn ancestor_scope_and_status_are_checked() {
    let original = manifest_for(&"a".repeat(40));
    let mut carried = carry(original, &"b".repeat(40), 8);
    carried.parent.as_mut().expect("parent").repository_id = digest_b3(b"foreign repo");
    assert_eq!(
        validate_manifest_lineage(&carried),
        Err("baseline_lineage_scope".to_owned())
    );

    let mut original = manifest_for(&"a".repeat(40));
    original.final_status = "failed".to_owned();
    let carried = carry(original, &"b".repeat(40), 8);
    assert_eq!(
        validate_manifest_lineage(&carried),
        Err("untrusted_proof".to_owned())
    );
}

#[test]
fn self_parent_and_overlong_chain_fail_closed() {
    let mut self_parent = manifest_for(&"a".repeat(40));
    self_parent.parent = Some(Box::new(self_parent.clone()));
    assert_eq!(
        validate_manifest_lineage(&self_parent),
        Err("baseline_lineage_cycle".to_owned())
    );

    let mut chain = manifest_for(&"a".repeat(40));
    for run_id in 8..39 {
        chain = carry(chain, &format!("{run_id:040x}"), run_id);
    }
    assert!(validate_manifest_lineage(&chain).is_ok());
    assert!(!baseline_can_carry(&chain));
    chain = carry(chain, &format!("{:040x}", 39), 39);
    assert_eq!(
        validate_manifest_lineage(&chain),
        Err("baseline_lineage_limit".to_owned())
    );
}

#[test]
fn missing_lineage_fields_and_malformed_digest_reject() {
    let manifest = manifest_for(&"a".repeat(40));
    let mut value = serde_json::to_value(&manifest).expect("manifest JSON");
    value
        .as_object_mut()
        .expect("manifest object")
        .remove("parent");
    assert!(serde_json::from_value::<BaselineManifest>(value).is_err());

    let mut value = serde_json::to_value(&manifest).expect("manifest JSON");
    value["tasks"][0]
        .as_object_mut()
        .expect("task object")
        .remove("carried_from");
    assert!(serde_json::from_value::<BaselineManifest>(value).is_err());

    let carried = carry(manifest, &"b".repeat(40), 8);
    let mut value = serde_json::to_value(&carried).expect("manifest JSON");
    value["tasks"][0]["carried_from"]["manifest_digest"] =
        serde_json::Value::String("malformed".to_owned());
    assert!(serde_json::from_value::<BaselineManifest>(value).is_err());
}

#[test]
fn expiry_cannot_be_reset_by_a_carrying_run() {
    let mut origin = manifest_for(&"a".repeat(40));
    origin.expires_at_unix = Some(100);
    let mut carried = carry(origin, &"b".repeat(40), 8);
    carried.expires_at_unix = None;
    assert_eq!(
        validate_manifest_lineage_at(&carried, 101),
        Err("cache_expired".to_owned())
    );
}

#[test]
fn lineage_bytes_stay_bounded() {
    let mut manifest = manifest_for(&"a".repeat(40));
    manifest.generator_version = "v".repeat(MAX_LINEAGE_BYTES);
    assert!(!baseline_can_carry(&manifest));
    assert_eq!(
        validate_manifest_lineage(&manifest),
        Err("baseline_lineage_limit".to_owned())
    );
}

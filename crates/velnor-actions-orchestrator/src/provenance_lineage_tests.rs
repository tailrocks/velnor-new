//! Protected carry ancestry regressions.
use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for, verdict};
use velnor_actions_contract::BaselineProof;

fn carry(parent: BaselineManifest, run_id: u64) -> BaselineManifest {
    let mut child = parent.clone();
    let digest = digest_b3(&canonical_json_bytes(&parent).expect("canonical"));
    child.run_id = run_id;
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
            .expect("binding"),
        );
    }
    child.parent = Some(Box::new(parent));
    child
}

#[test]
fn carried_origin_survives_multiple_successful_runs_and_merge() {
    let original = manifest_for(&"a".repeat(40));
    let carried = carry(carry(original, 8), 9);
    assert!(validate_manifest_lineage(&carried).is_ok());
    assert_eq!(carried.tasks[0].proof_run_id, 7);
    assert_eq!(carried.tasks[0].observed_run_id, 9);
    let plan = plan_for(&carried, Some(&carried.source_commit));
    assert!(!verdict(&plan, Some(&carried)).0.planning_failed);
}

#[test]
fn arbitrary_historical_run_without_parent_fails() {
    let mut manifest = manifest_for(&"a".repeat(40));
    manifest.tasks[0].proof_run_id = 5;
    assert_eq!(
        validate_manifest_lineage(&manifest),
        Err("originating_run_unverified".to_owned())
    );
}

#[test]
fn mutated_ancestor_and_task_proofs_fail_closed() {
    let carried = carry(manifest_for(&"a".repeat(40)), 8);
    let mut changed = carried.clone();
    changed.parent.as_mut().expect("parent").run_attempt = 2;
    assert!(
        validate_manifest_lineage(&changed).is_err(),
        "parent digest binds attempt"
    );
    let mut changed = carried.clone();
    changed.tasks[0].closure_digest = digest_b3(b"different closure");
    assert!(validate_manifest_lineage(&changed).is_err());
    let mut changed = carried.clone();
    changed.tasks[0].proof_run_id = 6;
    assert!(validate_manifest_lineage(&changed).is_err());
    let mut changed = carried;
    changed.tasks[0].observed_run_id = 7;
    assert!(validate_manifest_lineage(&changed).is_err());
}

#[test]
fn ancestor_scope_mismatch_and_failed_origin_fail() {
    let mutations: [fn(&mut BaselineManifest); 5] = [
        |node: &mut BaselineManifest| node.repository_id = digest_b3(b"foreign"),
        |node: &mut BaselineManifest| node.ref_ = "refs/heads/foreign".to_owned(),
        |node: &mut BaselineManifest| node.workflow_ref = "foreign/workflow".to_owned(),
        |node: &mut BaselineManifest| node.generator_sha256 = "2".repeat(64),
        |node: &mut BaselineManifest| node.final_status = "failed".to_owned(),
    ];
    for mutate in mutations {
        let mut origin = manifest_for(&"a".repeat(40));
        mutate(&mut origin);
        let mut child = carry(origin, 8);
        let trusted = manifest_for(&"a".repeat(40));
        child.repository_id = trusted.repository_id;
        child.ref_ = trusted.ref_;
        child.workflow_ref = trusted.workflow_ref;
        child.generator_sha256 = trusted.generator_sha256;
        child.final_status = trusted.final_status;
        assert!(validate_manifest_lineage(&child).is_err());
    }
}

#[test]
fn cycle_and_depth_bounds_force_refresh_before_publication() {
    let original = manifest_for(&"a".repeat(40));
    assert!(validate_manifest_lineage(&carry(original.clone(), 7)).is_err());
    let mut chain = original;
    for run_id in 8..38 {
        chain = carry(chain, run_id);
    }
    assert!(baseline_can_carry(&chain));
    chain = carry(chain, 38);
    assert!(validate_manifest_lineage(&chain).is_ok());
    assert!(!baseline_can_carry(&chain));
    chain = carry(chain, 39);
    assert_eq!(
        validate_manifest_lineage(&chain),
        Err("baseline_lineage_limit".to_owned())
    );
}

#[test]
fn missing_lineage_fields_require_migration() {
    let original = manifest_for(&"a".repeat(40));
    let mut value = serde_json::to_value(&original).expect("json");
    value.as_object_mut().expect("manifest").remove("parent");
    assert!(serde_json::from_value::<BaselineManifest>(value).is_err());
    let mut value = serde_json::to_value(&original).expect("json");
    value["tasks"][0]
        .as_object_mut()
        .expect("task")
        .remove("carried_from");
    assert!(serde_json::from_value::<BaselineManifest>(value).is_err());
}

#[test]
fn expired_origin_cannot_be_freshened_by_a_carrying_run() {
    let mut origin = manifest_for(&"a".repeat(40));
    origin.expires_at_unix = Some(100);
    let mut child = carry(origin, 8);
    child.expires_at_unix = None;
    assert_eq!(
        validate_manifest_lineage_at(&child, 101),
        Err("cache_expired".to_owned())
    );
}

#[test]
fn manifest_bytes_remain_bounded() {
    let mut manifest = manifest_for(&"a".repeat(40));
    manifest.generator_version = "v".repeat(MAX_LINEAGE_BYTES);
    assert!(!baseline_can_carry(&manifest));
    assert_eq!(
        validate_manifest_lineage(&manifest),
        Err("baseline_lineage_limit".to_owned())
    );
}

#[test]
fn plan_and_merge_accept_the_same_large_lineage() {
    use super::super::{ProvenanceExpectations, baseline_artifact_name, validate_provenance};

    let first = "a".repeat(40);
    let mut origin = manifest_for(&first);
    origin.repository_id = digest_b3(b"github.com/o/r");
    origin.tasks[0].external_data = Some(crate::external_data::ExternalDataFreshness {
        source: "x".repeat(20_000),
        identity: digest_b3(b"external snapshot"),
        age_secs: 0,
    });
    let mut carried = origin;
    for run_id in 8..39 {
        carried = carry(carried, run_id);
        carried.source_commit = format!("{run_id:040x}");
        carried.artifact_name =
            baseline_artifact_name(&carried.source_commit, &carried.compatibility_id)
                .expect("artifact name");
        carried.artifact_id =
            crate::cover_compat::baseline_artifact_numeric_id(&carried.artifact_name);
    }
    let size = canonical_json_bytes(&carried).expect("canonical").len();
    assert!(
        size > MAX_LINEAGE_BYTES / 2 && size < MAX_LINEAGE_BYTES,
        "{size}"
    );

    let expected = ProvenanceExpectations {
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
    assert!(validate_provenance(&carried, &digest, &expected).is_ok());
    let plan = plan_for(&carried, Some(&carried.source_commit));
    assert!(!verdict(&plan, Some(&carried)).0.planning_failed);
}

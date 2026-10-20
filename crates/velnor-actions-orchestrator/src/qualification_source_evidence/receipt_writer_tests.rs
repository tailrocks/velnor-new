use super::*;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendObservation, QualificationCacheLayerReceipt,
    QualificationCacheOwnerReceipt, QualificationCacheRestore, QualificationCacheRestoreResult,
    QualificationCacheSave, QualificationCacheSaveActionResult, QualificationRuntimeAbiObservation,
    QualificationRuntimeIdentity, QualificationRuntimeImageIdentity, QualificationRuntimePlatform,
    QualificationRuntimeToolObservation,
};

use crate::qualification_source_evidence::tests::{fixture, write_fixture_sidecar, write_reports};

#[test]
fn artifact_root_accepts_one_exact_extract_layout() {
    let temp = tempfile::tempdir().expect("temp");
    let reports = temp.path().join("reports");
    let marker = Path::new("qualification-cache/owners/job.json");
    let direct = reports.join("artifact-id").join(marker);
    std::fs::create_dir_all(direct.parent().expect("parent")).expect("directories");
    std::fs::write(&direct, b"{}").expect("marker");
    assert_eq!(
        artifact_root(&reports, "artifact-id", marker).expect("direct layout"),
        reports.join("artifact-id")
    );

    std::fs::remove_file(&direct).expect("remove marker");
    let nested = reports.join("artifact-id").join("artifact-id").join(marker);
    std::fs::create_dir_all(nested.parent().expect("parent")).expect("nested directories");
    std::fs::write(&nested, b"{}").expect("nested marker");
    assert_eq!(
        artifact_root(&reports, "artifact-id", marker).expect("nested layout"),
        reports.join("artifact-id").join("artifact-id")
    );
}

#[test]
fn artifact_root_rejects_missing_ambiguous_and_symlink_layouts() {
    let temp = tempfile::tempdir().expect("temp");
    let reports = temp.path().join("reports");
    std::fs::create_dir_all(&reports).expect("reports");
    let marker = Path::new("qualification-cache/owners/job.json");
    assert!(artifact_root(&reports, "missing", marker).is_err());

    let direct = reports.join("artifact-id").join(marker);
    let nested = reports.join("artifact-id").join("artifact-id").join(marker);
    std::fs::create_dir_all(direct.parent().expect("direct parent")).expect("direct dirs");
    std::fs::create_dir_all(nested.parent().expect("nested parent")).expect("nested dirs");
    std::fs::write(&direct, b"{}").expect("direct marker");
    std::fs::write(&nested, b"{}").expect("nested marker");
    assert!(artifact_root(&reports, "artifact-id", marker).is_err());

    let linked = temp.path().join("linked-reports");
    std::os::unix::fs::symlink(&reports, &linked).expect("reports symlink");
    assert!(artifact_root(&linked, "artifact-id", marker).is_err());
}

#[test]
fn canonical_reader_rejects_duplicate_keys_and_noncanonical_bytes() {
    assert!(parse_canonical::<serde_json::Value>(br#"{"a":1,"a":2}"#).is_err());
    assert!(parse_canonical::<serde_json::Value>(br#"{ "a": 1 }"#).is_err());
    assert_eq!(
        parse_canonical::<serde_json::Value>(br#"{"a":1}"#).expect("canonical value"),
        serde_json::json!({"a": 1})
    );
}

#[test]
fn plan_bound_artifact_assembly_accepts_owner_source_and_successful_reports() {
    let prepared = prepared_artifacts();
    let owners = read_owners(&prepared.reports, &prepared.plan, &prepared.directive)
        .expect("plan-bound owner artifact");
    let lanes = read_lanes(
        &prepared.reports,
        &prepared.plan,
        &prepared.directive,
        &prepared.authenticated,
        &owners,
    )
    .expect("plan-bound source and report artifacts");
    assert_eq!(owners, vec![prepared.owner]);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].matrix_key, prepared.entry.matrix_key);
    assert_eq!(lanes[0].task_id, prepared.entry.task_id);
    assert_eq!(
        lanes[0].closure_digest,
        prepared.source.evidence.closure.observed_closure_digest
    );
    assert_eq!(
        lanes[0].layers.len(),
        prepared.directive.owners[0].layers.len()
    );
    let document = velnor_actions_contract::workflow::QualificationCacheReceiptArtifactDocument {
        schema: 4,
        producer: prepared.authenticated.producer.clone(),
        evidence: velnor_actions_contract::workflow::QualificationCacheProducerEvidence {
            plan_id: prepared.plan.plan_id.clone(),
            campaign: prepared.directive.campaign.clone(),
            phase: prepared.directive.phase,
            source_sha: prepared.authenticated.producer.source_sha.clone(),
            configuration_digest: prepared.directive.configuration_digest.clone(),
            source_delta: None,
            predecessor: None,
            lanes,
            owners,
        },
    };
    let bytes = document
        .to_bounded_json(&prepared.plan, None)
        .expect("complete plan-bound receipt");
    assert!(bytes.len() < velnor_actions_contract::workflow::MAX_QUALIFICATION_RECEIPT_BYTES);
}

#[test]
fn receipt_assembly_rejects_owner_bound_to_another_job() {
    let prepared = prepared_artifacts();
    let mut wrong = prepared.owner.clone();
    wrong.owner_id = "other-job-owner".to_owned();
    write_owner(&prepared, &wrong);
    assert!(read_owners(&prepared.reports, &prepared.plan, &prepared.directive).is_err());
}

#[test]
fn receipt_assembly_rejects_source_evidence_bound_to_another_revision() {
    let prepared = prepared_artifacts();
    let mut wrong = prepared.source.clone();
    wrong.evidence.source_sha = "c".repeat(40);
    write_canonical(&source_path(&prepared), &wrong);
    let owners = read_owners(&prepared.reports, &prepared.plan, &prepared.directive)
        .expect("owner evidence remains valid");
    assert!(
        read_lanes(
            &prepared.reports,
            &prepared.plan,
            &prepared.directive,
            &prepared.authenticated,
            &owners,
        )
        .is_err()
    );
}

#[test]
fn receipt_assembly_rejects_missing_predecessor_admission() {
    let prepared = prepared_artifacts();
    let mut warm_plan = prepared.plan.clone();
    let qualification = warm_plan.qualification.as_mut().expect("qualification");
    qualification.phase = velnor_actions_contract::workflow::QualificationPhase::Warm;
    qualification.predecessor = Some(velnor_actions_contract::workflow::QualificationRunRef {
        run_id: qualification.run_id - 1,
        run_attempt: 1,
    });
    warm_plan.validate().expect("warm plan with predecessor");
    assert!(QualificationCacheDirective::for_plan(&warm_plan, None).is_err());
}

#[path = "receipt_writer_test_fixture.rs"]
mod fixtures;
use fixtures::*;

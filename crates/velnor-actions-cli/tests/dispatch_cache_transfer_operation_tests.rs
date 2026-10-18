use super::{RestoreActionObservation, export_output_body, import_output_body};
use crate::dispatch::cache_bind::transfer::{
    CacheExportProof, CacheRestoreProof, CacheRestoreStatus,
};
use std::path::PathBuf;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation,
};
use velnor_actions_orchestrator::QualificationCacheBindingOutputs;

const KEY: &str = "velnor-qcache-v2-campaign-1-mise_tools-runtime-k1";
const DIGEST: &str = "b3-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[test]
fn export_output_calls_payload_staged_and_not_backend_saved() {
    let proof = CacheExportProof {
        state_digest: DIGEST.to_owned(),
        save_key: Some(KEY.to_owned()),
        save_stage_path: Some(PathBuf::from("/tmp/velnor/stage/payload")),
        manifest_digest: Some(DIGEST.to_owned()),
    };
    let body = export_output_body(&proof).expect("export outputs");
    assert!(body.contains("snapshot_admitted=true\n"));
    assert!(body.contains("save_requested=true\n"));
    assert!(body.contains("export_result=staged\n"));
    assert!(!body.contains("export_result=written\n"));
}

#[test]
fn miss_output_admits_backend_only() {
    let binding = binding(None);
    let action = RestoreActionObservation {
        hit: false,
        matched_key: None,
    };
    let proof = CacheRestoreProof {
        status: CacheRestoreStatus::Miss,
        state_digest: None,
        stage_path: None,
    };
    let body = import_output_body(
        &binding,
        &action,
        &QualificationCacheBackendObservation::Absent,
        &proof,
    )
    .expect("miss outputs");
    assert!(body.contains("restore_result=miss\n"));
    assert!(body.contains("backend_admitted=true\n"));
    assert!(!body.contains("payload_admitted=true\n"));
    assert!(!body.contains("restore_admitted=true\n"));
}

#[test]
fn hit_output_admits_restore_only_with_completed_import_proof() {
    let entry = cache_entry();
    let binding = binding(Some(entry.clone()));
    let action = RestoreActionObservation {
        hit: true,
        matched_key: Some(KEY.to_owned()),
    };
    let proof = CacheRestoreProof {
        status: CacheRestoreStatus::Hit,
        state_digest: Some(DIGEST.to_owned()),
        stage_path: Some(PathBuf::from("/tmp/velnor/stage/payload")),
    };
    let body = import_output_body(
        &binding,
        &action,
        &QualificationCacheBackendObservation::Found(entry),
        &proof,
    )
    .expect("hit outputs");
    assert!(body.contains("backend_cache_id=71\n"));
    assert!(body.contains(&format!("backend_cache_version={}\n", "a".repeat(64))));
    assert!(body.ends_with("restore_admitted=true\n"));
}

fn binding(
    expected_cache: Option<QualificationCacheBackendEntry>,
) -> QualificationCacheBindingOutputs {
    QualificationCacheBindingOutputs {
        active: true,
        campaign: "campaign-1".to_owned(),
        phase: "cold".to_owned(),
        plan_id: "plan-1".to_owned(),
        run_key: "r10-a1".to_owned(),
        matrix_key: "lane-1".to_owned(),
        layer: "mise_tools".to_owned(),
        restore_key: Some(KEY.to_owned()),
        expected_cache,
        save_key: Some(KEY.to_owned()),
        save_if_state_changes: false,
        expected_prior_state_digest: Some(DIGEST.to_owned()),
        runtime_identity_digest: Some(DIGEST.to_owned()),
    }
}

fn cache_entry() -> QualificationCacheBackendEntry {
    QualificationCacheBackendEntry {
        id: 71,
        key: KEY.to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        version: "a".repeat(64),
        size_bytes: 512,
    }
}

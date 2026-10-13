use super::*;
use crate::{
    QUALIFICATION_CACHE_RECEIPT_ARTIFACT, QualificationCacheArtifact,
    QualificationCacheBackendEntry, QualificationCacheBackendObservation,
    QualificationCacheProducerContext, QualificationCacheReceipt, QualificationCacheRestore,
    QualificationCacheRestoreResult, QualificationCacheRunMetadata, QualificationCacheSave,
    QualificationCacheSaveActionResult,
};

#[test]
fn lanes_resolve_by_matrix_key_when_id_and_key_orders_differ() {
    let receipts = [
        lane("matrix-a", "closure-a", "useful-a", "state-a", false),
        lane("matrix-z", "closure-z", "useful-z", "state-z", false),
    ];
    let lookup = lane_index(&receipts).expect("unique matrix keys");
    let plan_id_order = ["matrix-z", "matrix-a"];
    assert_eq!(lookup[plan_id_order[0]].matrix_key, "matrix-z");
    assert_eq!(lookup[plan_id_order[1]].matrix_key, "matrix-a");
}

#[test]
fn useful_delta_needs_change_and_persistence_in_the_same_lane() {
    let previous = node(
        QualificationPhase::Third,
        vec![
            lane("changed", "closure-1", "useful-1", "state-1", false),
            lane("saved", "closure-2", "useful-2", "state-2", false),
        ],
    );
    let separated_evidence = node(
        QualificationPhase::UsefulDelta,
        vec![
            lane("changed", "closure-1b", "useful-1b", "state-1", false),
            lane("saved", "closure-2", "useful-2", "state-2b", true),
        ],
    );
    assert!(validate_useful_delta_progress(&separated_evidence, Some(&previous)).is_err());
    let same_lane = node(
        QualificationPhase::UsefulDelta,
        vec![
            lane("changed", "closure-1b", "useful-1b", "state-1b", true),
            lane("saved", "closure-2", "useful-2", "state-2", false),
        ],
    );
    assert!(validate_useful_delta_progress(&same_lane, Some(&previous)).is_ok());
}

fn lane(
    key: &str,
    closure: &str,
    useful: &str,
    state: &str,
    saved: bool,
) -> QualificationCacheLaneReceipt {
    let action = if saved {
        QualificationCacheSaveActionResult::Succeeded
    } else {
        QualificationCacheSaveActionResult::NotRequired
    };
    let after = if saved {
        QualificationCacheBackendObservation::Found(QualificationCacheBackendEntry {
            id: 8,
            key: "k3".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            size_bytes: 512,
        })
    } else {
        QualificationCacheBackendObservation::NotQueried
    };
    QualificationCacheLaneReceipt {
        matrix_key: key.to_owned(),
        stack_id: "rust".to_owned(),
        task_id: format!("task-{key}"),
        completed_task_ids: Vec::new(),
        closure_digest: crate::digest_b3(closure.as_bytes()),
        useful_state_digest: crate::digest_b3(useful.as_bytes()),
        layers: vec![QualificationCacheLayerReceipt {
            layer: QualificationCacheLayer::CargoSources,
            active: true,
            identity_digest: crate::digest_b3(b"identity"),
            runtime_identity: None,
            state_digest: Some(crate::digest_b3(state.as_bytes())),
            restore: QualificationCacheRestore {
                requested_key: Some("k2".to_owned()),
                matched_key: None,
                matched_cache: QualificationCacheBackendObservation::Absent,
                result: QualificationCacheRestoreResult::Miss,
            },
            save: QualificationCacheSave {
                requested_key: saved.then(|| "k3".to_owned()),
                action,
                before: if saved {
                    QualificationCacheBackendObservation::Absent
                } else {
                    QualificationCacheBackendObservation::NotQueried
                },
                after,
            },
        }],
    }
}

fn node(phase: QualificationPhase, lanes: Vec<QualificationCacheLaneReceipt>) -> AdmissionNode {
    let run = crate::workflow::QualificationRunRef {
        run_id: 12,
        run_attempt: 1,
    };
    AdmissionNode {
        metadata: QualificationCacheRunMetadata {
            repository: "owner/project".to_owned(),
            default_branch: "main".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            ref_protected: true,
            workflow_path_ref: ".github/workflows/ci.yml@main".to_owned(),
            workflow_ref: "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
            workflow_sha: "a".repeat(40),
            head_sha: "a".repeat(40),
            event: "workflow_dispatch".to_owned(),
            conclusion: "success".to_owned(),
            run,
        },
        artifact: QualificationCacheArtifact {
            id: 4,
            name: QUALIFICATION_CACHE_RECEIPT_ARTIFACT.to_owned(),
            digest: format!("sha256:{}", "b".repeat(64)),
            size_bytes: 512,
            expired: false,
            workflow_run_id: run.run_id,
            workflow_head_branch: "main".to_owned(),
            workflow_head_sha: "a".repeat(40),
        },
        producer: QualificationCacheProducerContext {
            repository: "owner/project".to_owned(),
            default_branch: "main".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            ref_protected: true,
            workflow_ref: "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
            workflow_sha: "a".repeat(40),
            source_sha: "a".repeat(40),
            run,
        },
        receipt: QualificationCacheReceipt {
            schema: 2,
            plan_id: "plan-r12-a1".to_owned(),
            run,
            campaign: "campaign".to_owned(),
            phase,
            source_sha: "a".repeat(40),
            configuration_digest: crate::digest_b3(b"configuration"),
            source_delta: None,
            predecessor: None,
            lanes,
        },
        previous: None,
    }
}

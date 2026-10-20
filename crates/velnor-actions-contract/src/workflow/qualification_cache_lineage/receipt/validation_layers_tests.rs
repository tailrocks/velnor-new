use super::*;
use crate::workflow::{
    QualificationCacheBackendEntry, QualificationCacheBackendObservation,
    QualificationCacheLayerReceipt, QualificationCacheOwnerReceipt, QualificationCacheReceipt,
    QualificationCacheRestore, QualificationCacheRestoreResult, QualificationCacheRunMetadata,
    QualificationCacheSave, QualificationCacheSaveActionResult,
};

#[test]
fn useful_delta_requires_an_owner_layer_change_and_persisted_save() {
    let previous = node(QualificationPhase::Third, owner("job-a", "state-a", false));
    let unchanged = owner("job-a", "state-a", true);
    let changed = owner("job-a", "state-b", true);
    let unchanged_lane = lane("closure-a", "useful-a");
    let changed_lane = lane("closure-b", "useful-b");
    assert!(
        validate_useful_delta_progress(&[unchanged_lane], &[unchanged], Some(&previous)).is_err()
    );
    assert!(validate_useful_delta_progress(&[changed_lane], &[changed], Some(&previous)).is_ok());
}

fn lane(closure: &str, useful: &str) -> crate::workflow::QualificationCacheLaneReceipt {
    crate::workflow::QualificationCacheLaneReceipt {
        matrix_key: "lane-a".to_owned(),
        stack_id: "rust".to_owned(),
        task_id: "task-a".to_owned(),
        completed_task_ids: Vec::new(),
        closure_digest: crate::digest_b3(closure.as_bytes()),
        useful_state_digest: crate::digest_b3(useful.as_bytes()),
        layers: Vec::new(),
    }
}

fn owner(owner_id: &str, state: &str, saved: bool) -> QualificationCacheOwnerReceipt {
    let found = QualificationCacheBackendObservation::Found(QualificationCacheBackendEntry {
        id: 8,
        key: "k3".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        version: "a".repeat(64),
        size_bytes: 512,
    });
    QualificationCacheOwnerReceipt {
        owner_id: owner_id.to_owned(),
        platform_id: crate::digest_b3(b"platform"),
        member_digest: crate::digest_b3(b"members"),
        members: vec![crate::workflow::QualificationCacheOwnerMemberRef {
            matrix_key: "lane-a".to_owned(),
            stack_id: "rust".to_owned(),
            task_id: "task-a".to_owned(),
            task_digest: crate::digest_b3(b"task"),
            input_digest: crate::digest_b3(b"input"),
        }],
        layers: vec![QualificationCacheLayerReceipt {
            owner_id: owner_id.to_owned(),
            platform_id: crate::digest_b3(b"platform"),
            member_digest: crate::digest_b3(b"members"),
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
                action: if saved {
                    QualificationCacheSaveActionResult::Succeeded
                } else {
                    QualificationCacheSaveActionResult::NotRequired
                },
                before: if saved {
                    QualificationCacheBackendObservation::Absent
                } else {
                    QualificationCacheBackendObservation::NotQueried
                },
                after: if saved {
                    found
                } else {
                    QualificationCacheBackendObservation::NotQueried
                },
            },
        }],
    }
}

fn node(phase: QualificationPhase, owner: QualificationCacheOwnerReceipt) -> AdmissionNode {
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
        artifact: super::super::super::types::QualificationCacheArtifact {
            id: 4,
            name: super::super::super::types::qualification_cache_receipt_artifact_name(run)
                .expect("The artifact name must be present."),
            digest: format!("sha256:{}", "b".repeat(64)),
            size_bytes: 512,
            expired: false,
            workflow_run_id: run.run_id,
            workflow_head_branch: "main".to_owned(),
            workflow_head_sha: "a".repeat(40),
        },
        producer: QualificationCacheProducerContext {
            mode: crate::workflow::QualificationDispatchMode::Controlled,
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
            schema: 4,
            plan_id: "plan-r12-a1".to_owned(),
            run,
            campaign: "campaign".to_owned(),
            phase,
            source_sha: "a".repeat(40),
            configuration_digest: crate::digest_b3(b"configuration"),
            source_delta: None,
            predecessor: None,
            lanes: vec![lane("closure-a", "useful-a")],
            owners: vec![owner],
        },
        previous: None,
    }
}

use super::{validate_document, verify_artifact_run};
use velnor_actions_contract::{
    QualificationCacheArtifact, QualificationCacheProducerContext, QualificationCacheReceipt,
    QualificationCacheReceiptArtifactDocument, QualificationCacheRunMetadata,
    QualificationDispatchMode, QualificationPhase, QualificationRunRef,
    qualification_cache_receipt_artifact_name,
};

#[test]
fn resolver_accepts_only_attempt_scoped_schema_three_documents() {
    let run = QualificationRunRef {
        run_id: 12,
        run_attempt: 1,
    };
    assert!(validate_document(&document(3, run), run).is_ok());
    assert!(validate_document(&document(2, run), run).is_err());

    let run_metadata = metadata(run);
    let old = artifact(run, "velnor-qualification-cache-receipt-v1");
    assert!(verify_artifact_run(&old, &run_metadata).is_err());
    let current_name = qualification_cache_receipt_artifact_name(run).expect("artifact name");
    let current = artifact(run, &current_name);
    assert!(verify_artifact_run(&current, &run_metadata).is_ok());

    let later_attempt = QualificationRunRef {
        run_id: run.run_id,
        run_attempt: 2,
    };
    let stale_attempt = artifact(
        later_attempt,
        &qualification_cache_receipt_artifact_name(run).expect("first attempt name"),
    );
    assert!(verify_artifact_run(&stale_attempt, &metadata(later_attempt)).is_err());
}

fn document(schema: u32, run: QualificationRunRef) -> QualificationCacheReceiptArtifactDocument {
    let source_sha = "a".repeat(40);
    QualificationCacheReceiptArtifactDocument {
        schema,
        producer: QualificationCacheProducerContext {
            mode: QualificationDispatchMode::Controlled,
            repository: "owner/project".to_owned(),
            default_branch: "main".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            ref_protected: true,
            workflow_ref: "owner/project/.github/workflows/ci.yml@refs/heads/main".to_owned(),
            workflow_sha: source_sha.clone(),
            source_sha: source_sha.clone(),
            run,
        },
        receipt: QualificationCacheReceipt {
            schema: 2,
            plan_id: "plan-r12-a1".to_owned(),
            run,
            campaign: "campaign".to_owned(),
            phase: QualificationPhase::Cold,
            source_sha,
            configuration_digest: format!("b3-{}", "b".repeat(64)),
            source_delta: None,
            predecessor: None,
            lanes: Vec::new(),
        },
    }
}

fn metadata(run: QualificationRunRef) -> QualificationCacheRunMetadata {
    QualificationCacheRunMetadata {
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
    }
}

fn artifact(run: QualificationRunRef, name: &str) -> QualificationCacheArtifact {
    QualificationCacheArtifact {
        id: 4,
        name: name.to_owned(),
        digest: format!("sha256:{}", "c".repeat(64)),
        size_bytes: 512,
        expired: false,
        workflow_run_id: run.run_id,
        workflow_head_branch: "main".to_owned(),
        workflow_head_sha: "a".repeat(40),
    }
}

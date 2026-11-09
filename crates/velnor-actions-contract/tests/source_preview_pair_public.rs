// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::source_preview::{
    SourcePreviewLock, SourcePreviewPayload, SourcePreviewProducer, SourcePreviewReceipt,
    SourcePreviewRun, SourcePreviewTarget, SourcePreviewTargets,
};

fn preview() -> SourcePreviewLock {
    let source = "a".repeat(40);
    let run = |path: &str, reference: &str, id| SourcePreviewRun {
        workflow_path: path.to_owned(),
        workflow_ref: format!("tailrocks/velnor-new/{path}@{reference}"),
        workflow_sha: source.clone(),
        run_id: id,
        run_attempt: 1,
    };
    let target = |name: &str, original, retained, job| SourcePreviewTarget {
        producer: SourcePreviewProducer {
            producer_run_id: 4,
            producer_run_attempt: 1,
            payload_artifact_id: original,
            payload_archive_sha256: "e".repeat(64),
        },
        candidate_payload_artifact_id: retained,
        qualification_job_id: job,
        payload: SourcePreviewPayload {
            target: name.to_owned(),
            name: format!("velnor-actions-{name}"),
            size: 12,
            sha256: "f".repeat(64),
        },
    };
    let tag_ref = format!("refs/tags/sdk-preview-{source}");
    SourcePreviewLock {
        receipt: SourcePreviewReceipt {
            schema: 3,
            repository: "tailrocks/velnor-new".to_owned(),
            version: "0.1.78".to_owned(),
            source_sha: source.clone(),
            tag: format!("sdk-preview-{source}"),
            release_id: 1,
            source: run(
                ".github/workflows/product-release-generator.yml",
                &tag_ref,
                1,
            ),
            draft: run(
                ".github/workflows/product-release-generator.yml",
                &tag_ref,
                1,
            ),
            signer: run(
                ".github/workflows/product-release-generator.yml",
                &tag_ref,
                1,
            ),
            ci: run(".github/workflows/ci.yml", "refs/heads/main", 2),
            qualification: run(
                ".github/workflows/source-qualification.yml",
                "refs/heads/main",
                3,
            ),
            candidate_source_artifact_id: 5,
            targets: SourcePreviewTargets {
                linux: target("x86_64-unknown-linux-gnu", 6, 7, 8),
                darwin: target("aarch64-apple-darwin", 9, 10, 11),
            },
        },
        receipt_sha256: "b".repeat(64),
        bundle_sha256: "c".repeat(64),
        publisher_run_attempt: 1,
        publisher_job_id: 12,
    }
}

#[test]
fn source_preview_requires_distinct_fixed_target_custody() {
    let valid = preview();
    assert!(valid.validate_shape().is_ok());
    let mut wrong = valid.clone();
    wrong.receipt.targets.darwin.payload.target = "x86_64-unknown-linux-gnu".to_owned();
    assert!(
        wrong.validate_shape().is_err(),
        "wrong Darwin target accepted"
    );
    let mut wrong = valid.clone();
    wrong.receipt.targets.darwin.producer.payload_artifact_id =
        wrong.receipt.targets.linux.producer.payload_artifact_id;
    assert!(wrong.validate_shape().is_err(), "shared artifact accepted");
    let mut wrong = valid.clone();
    wrong.receipt.targets.darwin.qualification_job_id =
        wrong.receipt.targets.linux.qualification_job_id;
    assert!(
        wrong.validate_shape().is_err(),
        "shared qualification accepted"
    );
    let mut wrong = valid;
    wrong.receipt.targets.darwin.payload.sha256 = "INVALID".to_owned();
    assert!(wrong.validate_shape().is_err(), "invalid digest accepted");
}

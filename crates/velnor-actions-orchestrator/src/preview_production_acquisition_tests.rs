//! Canonical step-output binding is independent of mutable local metadata.
use super::*;
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    preview_production::{
        PayloadArtifactRef, RELEASE_PRODUCTION_SCHEMA_V1, ReleaseProductionContext,
        expected_release_payload_pairs, release_plan_sha256,
    },
};

fn request() -> ReleaseProductionRequest {
    let plan = PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.into(),
        channel: PackageReleaseChannel::Preview,
        repository: "owner/repo".into(),
        default_branch: "main".into(),
        base_version: "1.2.3".into(),
        products: vec![PackageReleasePlanProduct {
            id: "cli".into(),
            package: "cli".into(),
            bins: vec!["cli".into()],
            targets: vec!["aarch64-apple-darwin".into(), "x86_64-apple-darwin".into()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![],
    };
    let inputs = expected_release_payload_pairs(&plan)
        .expect("pairs")
        .into_iter()
        .enumerate()
        .map(|(index, pair)| PayloadArtifactRef {
            pair,
            artifact_id: index as u64 + 100,
            sha256: "b".repeat(64),
        })
        .collect();
    ReleaseProductionRequest {
        schema: RELEASE_PRODUCTION_SCHEMA_V1.into(),
        policy_sha256: "d".repeat(64),
        plan_sha256: release_plan_sha256(&plan).expect("digest"),
        plan,
        context: ReleaseProductionContext {
            repository_id: 1,
            head_repository_id: 1,
            source_sha: "a".repeat(40),
            source_ref: "refs/heads/main".into(),
            workflow_authority_sha: "a".repeat(40),
            run_id: 3,
            run_attempt: 1,
            ci_run_id: 2,
            ci_run_attempt: 1,
            commit_count: 10,
            version: "1.2.3-preview.10+aaaaaaa".into(),
        },
        stage: ReleaseProductionStage::Assemble { inputs },
    }
}

fn admission() -> Admission {
    Admission {
        schema: SCHEMA.to_owned(),
        request: request(),
        leaves: vec![],
        inputs: vec![],
    }
}

#[test]
fn handoff_digest_rejects_missing_changed_and_noncanonical_record() {
    let record = admission();
    let bytes = canonical_json_bytes(&record).expect("record");
    let digest = handoff::sha256(&bytes);
    assert!(parse(&bytes, &digest, &record.request).is_ok());
    assert!(parse(&bytes, "", &record.request).is_err());
    let mut changed = bytes.clone();
    changed.push(b' ');
    assert!(parse(&changed, &digest, &record.request).is_err());
    assert!(parse(&changed, &handoff::sha256(&changed), &record.request).is_err());
    let mut altered = admission();
    altered.request.context.run_attempt += 1;
    let altered = canonical_json_bytes(&altered).expect("altered");
    assert!(parse(&altered, &handoff::sha256(&altered), &record.request).is_err());
}

#[test]
fn source_executor_refuses_either_ambient_token() {
    assert!(credentials_absent(false).is_ok());
    assert!(credentials_absent(true).is_err());
}

#[test]
fn admitted_union_cannot_omit_original_leaf_proofs() {
    let deadline =
        CheckDeadline::from_start(Instant::now(), Duration::from_secs(5)).expect("deadline");
    let preview = RustBinaryReleaseConfig::default();
    let error = restore(admission(), &preview, deadline)
        .err()
        .expect("missing leaf rejected");
    assert!(
        error.to_string().contains("preview_admission_inventory"),
        "{error}"
    );
}

fn admitted_files(root: &Path, preview: &RustBinaryReleaseConfig) -> Admission {
    let mut record = admission();
    let ReleaseProductionStage::Assemble { inputs } = record.request.stage.clone() else {
        panic!("assemble")
    };
    for reference in inputs {
        let mut request = record.request.clone();
        request.stage = ReleaseProductionStage::Payload {
            pair: reference.pair.clone(),
        };
        let producer = super::super::PreviewProducerProof {
            source_sha: request.context.source_sha.clone(),
            source_ref: request.context.source_ref.clone(),
            version: request.context.version.clone(),
            commit_count: request.context.commit_count,
            task: preview.producer.task.clone(),
            task_exit_code: 0,
            duration_ms: 1,
            output_directory: preview.producer.output_directory.clone(),
            output_manifest: preview.producer.manifest.clone(),
            qualification_digest: "fixture-not-qualification".into(),
            system_tools: vec![],
            qualified_tools: vec![],
        };
        let archive = format!(
            "{}-{}.tar.gz",
            reference.pair.product_id, reference.pair.target
        );
        let mut assets = Vec::new();
        for name in [archive.clone(), format!("{archive}.sbom.json")] {
            let path = root.join(&name);
            std::fs::write(&path, b"abc").expect("fixture bytes");
            let proof = handoff::AssetProof {
                name: name.clone(),
                sha256: handoff::sha256(b"abc"),
                size: 3,
            };
            record.inputs.push(TaskInput {
                pair: reference.pair.clone(),
                name,
                sha256: proof.sha256.clone(),
                size: 3,
                path: path.to_str().expect("path").into(),
            });
            assets.push(proof);
        }
        assets.sort_by(|a, b| a.name.cmp(&b.name));
        record.leaves.push(PreviewLeafManifest {
            schema: leaf_manifest::LEAF_SCHEMA.into(),
            request,
            producer,
            assets,
        });
    }
    record.inputs.sort_by(|a, b| a.name.cmp(&b.name));
    record
}

#[test]
fn complete_admission_repins_assets_and_rejects_pre_task_byte_tampering() {
    let root = tempfile::tempdir().expect("root");
    let root = root.path().canonicalize().expect("canonical root");
    let preview = RustBinaryReleaseConfig::default();
    let deadline =
        CheckDeadline::from_start(Instant::now(), Duration::from_secs(5)).expect("deadline");
    let record = admitted_files(&root, &preview);
    let bytes = canonical_json_bytes(&record).expect("record");
    let restored =
        parse(&bytes, &handoff::sha256(&bytes), &record.request).expect("digest admission");
    let pinned = restore(restored, &preview, deadline).expect("full closure");
    assert_eq!(pinned.inputs.len(), 4);
    std::fs::write(&pinned.inputs[0].path, b"bad").expect("tamper");
    assert!(pinned.recheck(deadline).is_err());
    let restored =
        parse(&bytes, &handoff::sha256(&bytes), &record.request).expect("unchanged metadata");
    assert!(
        restore(restored, &preview, deadline).is_err(),
        "immutable metadata cannot hide changed assets"
    );
}

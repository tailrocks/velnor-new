//! Full-plan native evidence checks; these fixtures make no remote-auth claim.
use super::*;
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    preview_production::{
        PREVIEW_PRODUCTION_SCHEMA_V1, PayloadArtifactRef, PreviewProductionContext,
        expected_preview_payload_pairs, preview_plan_sha256,
    },
};

fn request() -> PreviewProductionRequest {
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
    let inputs = expected_preview_payload_pairs(&plan)
        .expect("pairs")
        .into_iter()
        .enumerate()
        .map(|(index, pair)| PayloadArtifactRef {
            pair,
            artifact_id: index as u64 + 100,
            sha256: "b".repeat(64),
        })
        .collect();
    PreviewProductionRequest {
        schema: PREVIEW_PRODUCTION_SCHEMA_V1.into(),
        plan_sha256: preview_plan_sha256(&plan).expect("digest"),
        plan,
        context: PreviewProductionContext {
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
        stage: PreviewProductionStage::Assemble { inputs },
    }
}

fn producer(request: &PreviewProductionRequest) -> PreviewProducerProof {
    PreviewProducerProof {
        source_sha: request.context.source_sha.clone(),
        source_ref: request.context.source_ref.clone(),
        version: request.context.version.clone(),
        commit_count: request.context.commit_count,
        task: "preview".into(),
        task_exit_code: 0,
        duration_ms: 1,
        output_directory: "dist/preview".into(),
        output_manifest: "dist/preview/package-output.json".into(),
        qualification_digest: "fixture-not-qualification".into(),
        system_tools: vec![],
        qualified_tools: vec![],
    }
}

fn aggregate() -> PreviewAggregateManifest {
    let request = request();
    let PreviewProductionStage::Assemble { inputs } = &request.stage else {
        panic!("assembly")
    };
    let leaves = inputs
        .iter()
        .map(|input| {
            let mut leaf_request = request.clone();
            leaf_request.stage = PreviewProductionStage::Payload {
                pair: input.pair.clone(),
            };
            let archive = format!("{}-{}.tar.gz", input.pair.product_id, input.pair.target);
            let mut assets = vec![
                AssetProof {
                    name: archive.clone(),
                    sha256: "c".repeat(64),
                    size: 3,
                },
                AssetProof {
                    name: format!("{archive}.sbom.json"),
                    sha256: "d".repeat(64),
                    size: 4,
                },
            ];
            assets.sort_by(|a, b| a.name.cmp(&b.name));
            PreviewLeafManifest {
                schema: LEAF_SCHEMA.into(),
                producer: producer(&leaf_request),
                request: leaf_request,
                assets,
            }
        })
        .collect::<Vec<_>>();
    let mut assets = leaves
        .iter()
        .flat_map(|leaf| leaf.assets.clone())
        .collect::<Vec<_>>();
    assets.sort_by(|a, b| a.name.cmp(&b.name));
    PreviewAggregateManifest {
        schema: AGGREGATE_SCHEMA.into(),
        producer: producer(&request),
        request,
        leaves,
        assets,
    }
}

fn accepted(proof: &PreviewAggregateManifest) -> bool {
    PreviewAggregateManifest::parse(
        &encode(proof).expect("canonical"),
        &proof.request.plan,
        &proof.request.context,
    )
    .is_ok()
}

#[test]
fn native_union_rejects_missing_duplicated_or_mutated_leaf_bytes() {
    let proof = aggregate();
    assert!(accepted(&proof));
    let mut missing = aggregate();
    missing.leaves.pop();
    assert!(!accepted(&missing));
    let mut duplicate = aggregate();
    duplicate.leaves[1] = duplicate.leaves[0].clone();
    assert!(!accepted(&duplicate));
    for field in ["hash", "size"] {
        let mut changed = aggregate();
        if field == "hash" {
            changed.assets[0].sha256 = "e".repeat(64);
        } else {
            changed.assets[0].size += 1;
        }
        assert!(!accepted(&changed));
    }
}

#[test]
fn native_leaf_rejects_subset_plan_context_role_and_failed_task_evidence() {
    let mut proof = aggregate();
    let expected = proof.leaves[0].request.clone();
    let leaf = &mut proof.leaves[0];
    PreviewLeafManifest::parse(&encode(leaf).expect("encode"), &expected).expect("leaf");
    leaf.request.plan.products[0].targets.pop();
    leaf.request.plan_sha256 = preview_plan_sha256(&leaf.request.plan).expect("subset digest");
    assert!(PreviewLeafManifest::parse(&encode(leaf).expect("encode"), &expected).is_err());
    let mut failed = aggregate().leaves.remove(0);
    failed.producer.task_exit_code = 17;
    assert!(PreviewLeafManifest::parse(&encode(&failed).expect("encode"), &expected).is_err());
    let mut replay = aggregate().leaves.remove(0);
    replay.request.context.run_attempt += 1;
    assert!(PreviewLeafManifest::parse(&encode(&replay).expect("encode"), &expected).is_err());
}

#[test]
fn native_metadata_is_canonical_and_rejects_unknown_fields() {
    let proof = aggregate();
    let bytes = encode(&proof).expect("encode");
    let mut noncanonical = bytes.clone();
    noncanonical.push(b'\n');
    assert!(
        PreviewAggregateManifest::parse(&noncanonical, &proof.request.plan, &proof.request.context)
            .is_err()
    );
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    value["verified"] = serde_json::json!(true);
    assert!(
        PreviewAggregateManifest::parse(
            &velnor_actions_contract::canonical_json_bytes(&value).expect("canonical"),
            &proof.request.plan,
            &proof.request.context
        )
        .is_err()
    );
}

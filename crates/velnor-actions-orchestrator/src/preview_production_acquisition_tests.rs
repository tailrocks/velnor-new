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
            workflow_authority_sha: "b".repeat(40),
            run_id: 3,
            run_attempt: 1,
            ci_run_id: 2,
            ci_run_attempt: 1,
            commit_count: 10,
            version: "1.2.3-preview.10+aaaaaaa".into(),
        },
        stage: ReleaseProductionStage::Assemble { inputs: Vec::new() },
    }
}

fn completed_request() -> ReleaseProductionRequest {
    let mut request = request();
    let pairs = expected_release_payload_pairs(&request.plan).expect("pairs");
    request.stage = ReleaseProductionStage::Assemble {
        inputs: pairs
            .into_iter()
            .enumerate()
            .map(|(index, pair)| PayloadArtifactRef {
                pair,
                artifact_id: index as u64 + 100,
                sha256: "b".repeat(64),
            })
            .collect(),
    };
    request
}

fn admission() -> Admission {
    Admission {
        schema: SCHEMA.to_owned(),
        request: completed_request(),
        leaves: vec![],
        inputs: vec![],
    }
}

#[test]
fn handoff_digest_rejects_missing_changed_and_noncanonical_record() {
    let record = admission();
    let bytes = canonical_json_bytes(&record).expect("record");
    let digest = handoff::sha256(&bytes);
    assert!(parse(&bytes, &digest, &request()).is_ok());
    assert!(parse(&bytes, "", &request()).is_err());
    let mut changed = bytes.clone();
    changed.push(b' ');
    assert!(parse(&changed, &digest, &request()).is_err());
    assert!(parse(&changed, &handoff::sha256(&changed), &request()).is_err());
    let mut altered = admission();
    altered.request.context.run_attempt += 1;
    let altered = canonical_json_bytes(&altered).expect("altered");
    assert!(parse(&altered, &handoff::sha256(&altered), &request()).is_err());
}

#[test]
fn stable_caller_must_match_the_entire_original_request() {
    let mut stable = admission();
    stable.request.plan.channel =
        velnor_actions_contract::package_release::PackageReleaseChannel::Stable;
    stable.request.plan_sha256 =
        velnor_actions_contract::preview_production::release_plan_sha256(&stable.request.plan)
            .expect("stable plan digest");
    let exact = stable.request.clone();
    assert!(caller_request_matches(&stable, &exact));
    let mut changed = exact;
    let ReleaseProductionStage::Assemble { inputs } = &mut changed.stage else {
        panic!("assembly")
    };
    inputs[0].sha256 = "e".repeat(64);
    assert!(!caller_request_matches(&stable, &changed));
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

fn admitted_files(
    root: &Path,
    preview: &RustBinaryReleaseConfig,
    request: ReleaseProductionRequest,
) -> Admission {
    let mut record = Admission {
        schema: SCHEMA.to_owned(),
        request,
        leaves: Vec::new(),
        inputs: Vec::new(),
    };
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
    let record = admitted_files(&root, &preview, completed_request());
    let bytes = canonical_json_bytes(&record).expect("record");
    let restored = parse(&bytes, &handoff::sha256(&bytes), &request()).expect("digest admission");
    let pinned = restore(restored, &preview, deadline).expect("full closure");
    assert_eq!(pinned.inputs.len(), 4);
    assert_eq!(pinned.request.stage, completed_request().stage);
    let task_root = tempfile::tempdir().expect("task request root");
    let task_root = task_root.path().canonicalize().expect("task root");
    let task = handoff::write_task_request(&task_root, &pinned.request, &pinned.inputs, deadline)
        .expect("complete sealed union reaches canonical producer handoff");
    let task_bytes = std::fs::read(task.path).expect("task request bytes");
    let task_value: serde_json::Value = serde_json::from_slice(&task_bytes).expect("task JSON");
    assert_eq!(
        task_value["request"]["context"]["source_sha"],
        "a".repeat(40)
    );
    assert_eq!(
        task_value["request"]["context"]["workflow_authority_sha"],
        "b".repeat(40)
    );
    assert_eq!(
        task_value["request"]["stage"]["inputs"]
            .as_array()
            .expect("union")
            .len(),
        2
    );
    assert_eq!(
        task_value["inputs"]
            .as_array()
            .expect("pinned inputs")
            .len(),
        4
    );
    std::fs::write(&pinned.inputs[0].path, b"bad").expect("tamper");
    assert!(pinned.recheck(deadline).is_err());
    let restored = parse(&bytes, &handoff::sha256(&bytes), &request()).expect("unchanged metadata");
    assert!(
        restore(restored, &preview, deadline).is_err(),
        "immutable metadata cannot hide changed assets"
    );
}

#[test]
fn discovered_transport_union_is_the_sealed_and_handed_off_union() {
    let root = tempfile::tempdir().expect("root");
    let root = root.path().canonicalize().expect("canonical root");
    let preview = RustBinaryReleaseConfig::default();
    let deadline =
        CheckDeadline::from_start(Instant::now(), Duration::from_secs(5)).expect("deadline");
    let original = request();
    let completed = crate::package_release_publish::discover_request_for_sealing_test(&original)
        .expect("authenticated API inventory returns full request");
    let ReleaseProductionStage::Assemble { inputs } = &completed.stage else {
        panic!("completed assembly")
    };
    assert_eq!(inputs.len(), 2);
    assert_eq!(inputs[0].artifact_id, 303);
    assert_eq!(inputs[0].sha256, handoff::sha256(b"closed ZIP bytes"));

    let record = admitted_files(&root, &preview, completed.clone());
    let expected_request = record.request.clone();
    let restored = restore(record, &preview, deadline).expect("valid source-bound leaves");
    let mut acquiring = restored;
    acquiring.request = original.clone();
    let sealed =
        super::super::aggregate::complete_acquisition(acquiring, &original, completed, deadline)
            .expect("production aggregate completion stores API-discovered request");
    let sealed_record = Admission {
        schema: SCHEMA.to_owned(),
        request: sealed.request,
        leaves: sealed.manifests,
        inputs: sealed.inputs,
    };
    let sealed_bytes = canonical_json_bytes(&sealed_record).expect("sealed record");
    let sealed_digest = handoff::sha256(&sealed_bytes);
    let parsed = parse(&sealed_bytes, &sealed_digest, &original).expect("sealed record parses");
    let pinned = restore(parsed, &preview, deadline).expect("restored exact union");
    assert_eq!(pinned.request, expected_request);
    let task_root = tempfile::tempdir().expect("task request root");
    let task_root = task_root.path().canonicalize().expect("task root");
    let task = handoff::write_task_request(&task_root, &pinned.request, &pinned.inputs, deadline)
        .expect("canonical task request");
    let task_value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(task.path).expect("handoff bytes"))
            .expect("task JSON");
    let task_refs = task_value["request"]["stage"]["inputs"]
        .as_array()
        .expect("task request refs");
    assert_eq!(task_refs.len(), 2);
    assert_eq!(task_refs[0]["artifact_id"], 303);
    assert_eq!(task_refs[0]["sha256"], handoff::sha256(b"closed ZIP bytes"));
    assert_eq!(task_refs[0]["pair"]["product_id"], "cli");
    assert_eq!(task_refs[1]["artifact_id"], 304);
    assert_eq!(task_refs[1]["sha256"], handoff::sha256(b"closed ZIP bytes"));
    assert_eq!(task_value["inputs"].as_array().expect("inputs").len(), 4);
}

//! Closed needs and role checks without process-global environment mutation.
use super::*;
use serde_json::{Value, json};
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
};

fn fixture() -> ReleaseProductionRequest {
    let plan = PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "owner/repo".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: vec![PackageReleasePlanProduct {
            id: "cli".to_owned(),
            package: "cli".to_owned(),
            bins: vec!["cli".to_owned()],
            targets: vec![
                "aarch64-apple-darwin".to_owned(),
                "x86_64-apple-darwin".to_owned(),
            ],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: Vec::new(),
    };
    ReleaseProductionRequest {
        schema: RELEASE_PRODUCTION_SCHEMA_V1.to_owned(),
        policy_sha256: "d".repeat(64),
        plan_sha256: release_plan_sha256(&plan).expect("digest"),
        plan,
        context: ReleaseProductionContext {
            repository_id: 1,
            head_repository_id: 1,
            source_sha: "a".repeat(40),
            source_ref: "refs/heads/main".to_owned(),
            workflow_authority_sha: "a".repeat(40),
            run_id: 3,
            run_attempt: 1,
            ci_run_id: 2,
            ci_run_attempt: 1,
            commit_count: 10,
            version: "1.2.3-preview.10+aaaaaaa".to_owned(),
        },
        stage: ReleaseProductionStage::Assemble { inputs: Vec::new() },
    }
}

fn stable_fixture() -> ReleaseProductionRequest {
    let mut request = fixture();
    request.plan.channel = PackageReleaseChannel::Stable;
    request.plan_sha256 = release_plan_sha256(&request.plan).expect("stable digest");
    request.context.commit_count = 0;
    request.context.version = request.plan.base_version.clone();
    request
}

fn stable_origin_outputs() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("origin_artifact_id".to_owned(), "43".to_owned()),
        ("origin_artifact_digest".to_owned(), "c".repeat(64)),
        ("intent_artifact_id".to_owned(), "44".to_owned()),
        ("intent_artifact_digest".to_owned(), "d".repeat(64)),
    ])
}

fn needs(request: &ReleaseProductionRequest) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(
        match request.plan.channel {
            PackageReleaseChannel::Preview => "preview-eligibility",
            PackageReleaseChannel::Stable => "stable-tag",
        }
        .to_owned(),
        json!({"result":"success", "outputs":{
            "source_sha":request.context.source_sha,
            "workflow_authority_sha":request.context.workflow_authority_sha,
            "ci_run_id":request.context.ci_run_id.to_string(),
            "ci_attempt":request.context.ci_run_attempt.to_string(),
            "commit_count":request.context.commit_count.to_string()
        }}),
    );
    if request.plan.channel == PackageReleaseChannel::Stable {
        let outputs = map.get_mut("stable-tag").expect("stable tag need");
        let mut values = outputs["outputs"].as_object().expect("output map").clone();
        for (key, value) in stable_origin_outputs() {
            values.insert(key, json!(value));
        }
        outputs["outputs"] = Value::Object(values);
    }
    for (index, pair) in expected_release_payload_pairs(&request.plan)
        .expect("pairs")
        .iter()
        .enumerate()
    {
        map.insert(
            release_payload_job_id(&request.plan, pair).expect("job"),
            json!({"result":"success", "outputs":{
                "artifact_id":(index+100).to_string(), "artifact_digest":"b".repeat(64)
            }}),
        );
    }
    Value::Object(map)
}

fn parse(
    request: &ReleaseProductionRequest,
    value: &Value,
) -> Result<Vec<PayloadArtifactRef>, OrchestratorError> {
    let additional = if request.plan.channel == PackageReleaseChannel::Stable {
        stable_origin_outputs()
    } else {
        BTreeMap::new()
    };
    parse_needs_with_outputs(
        &value.to_string(),
        &request.plan,
        &request.context,
        &additional,
    )
}

#[test]
fn needs_requires_full_success_union_and_exact_eligibility_identity() {
    let request = stable_fixture();
    let good = needs(&request);
    assert_eq!(parse(&request, &good).expect("union").len(), 2);
    let jobs = expected_release_payload_pairs(&request.plan)
        .expect("pairs")
        .iter()
        .map(|pair| release_payload_job_id(&request.plan, pair).expect("job"))
        .collect::<Vec<_>>();
    for job in &jobs {
        let mut missing = good.clone();
        missing.as_object_mut().expect("map").remove(job);
        assert!(parse(&request, &missing).is_err());
        for status in ["failure", "cancelled", "skipped", ""] {
            let mut failed = good.clone();
            failed[job]["result"] = json!(status);
            assert!(parse(&request, &failed).is_err());
        }
    }
    let mut extra = good.clone();
    extra["unknown"] = json!({"result":"success","outputs":{}});
    assert!(parse(&request, &extra).is_err());
    for key in [
        "source_sha",
        "workflow_authority_sha",
        "ci_run_id",
        "ci_attempt",
        "commit_count",
    ] {
        let mut wrong = good.clone();
        wrong["stable-tag"]["outputs"][key] = json!("wrong");
        assert!(parse(&request, &wrong).is_err());
    }
}

#[test]
fn needs_rejects_duplicate_json_noncanonical_ids_and_request_duplicate_artifacts() {
    let mut request = stable_fixture();
    let good = needs(&request);
    assert!(
        parse_needs_with_outputs(
            "{\"x\":{},\"x\":{}}",
            &request.plan,
            &request.context,
            &stable_origin_outputs()
        )
        .is_err()
    );
    let pairs = expected_release_payload_pairs(&request.plan).expect("pairs");
    for value in ["0", "01", "+1", "-1", "18446744073709551616"] {
        let mut bad = good.clone();
        let first = pairs.first().expect("first pair");
        let job = release_payload_job_id(&request.plan, first).expect("first job");
        bad[&job]["outputs"]["artifact_id"] = json!(value);
        assert!(parse(&request, &bad).is_err());
    }
    let mut duplicated = good.clone();
    let second = pairs.get(1).expect("second pair");
    let job = release_payload_job_id(&request.plan, second).expect("second job");
    duplicated[&job]["outputs"]["artifact_id"] = json!("100");
    request.stage = ReleaseProductionStage::Assemble {
        inputs: parse(&request, &duplicated).expect("syntax"),
    };
    assert!(request.validate().is_err());
}

#[test]
fn job_role_is_bound_to_selected_pair_or_complete_assembler() {
    let mut request = fixture();
    assert!(validate_role(&request, "preview-assemble").is_ok());
    assert!(validate_role(&request, "preview-signer").is_err());
    request.stage = ReleaseProductionStage::Payload {
        pair: expected_release_payload_pairs(&request.plan)
            .expect("pairs")
            .remove(0),
    };
    assert!(validate_role(&request, "preview-payload").is_ok());
    for role in [
        "preview-payload-000",
        "preview-assemble",
        "preview-signer",
        "",
    ] {
        assert!(validate_role(&request, role).is_err());
    }
}

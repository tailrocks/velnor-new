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

fn needs(request: &ReleaseProductionRequest) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(
        "preview-eligibility".to_owned(),
        json!({"result":"success", "outputs":{
            "source_sha":request.context.source_sha,
            "workflow_authority_sha":request.context.workflow_authority_sha,
            "ci_run_id":"2", "ci_attempt":"1", "commit_count":"10"
        }}),
    );
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
    parse_needs(&value.to_string(), &request.plan, &request.context)
}

#[test]
fn needs_requires_full_success_union_and_exact_eligibility_identity() {
    let request = fixture();
    let good = needs(&request);
    assert_eq!(parse(&request, &good).expect("union").len(), 2);
    for job in ["preview-payload-000", "preview-payload-001"] {
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
        wrong["preview-eligibility"]["outputs"][key] = json!("wrong");
        assert!(parse(&request, &wrong).is_err());
    }
}

#[test]
fn needs_rejects_duplicate_json_noncanonical_ids_and_request_duplicate_artifacts() {
    let mut request = fixture();
    let good = needs(&request);
    assert!(parse_needs("{\"x\":{},\"x\":{}}", &request.plan, &request.context).is_err());
    for value in ["0", "01", "+1", "-1", "18446744073709551616"] {
        let mut bad = good.clone();
        bad["preview-payload-000"]["outputs"]["artifact_id"] = json!(value);
        assert!(parse(&request, &bad).is_err());
    }
    let mut duplicated = good.clone();
    duplicated["preview-payload-001"]["outputs"]["artifact_id"] = json!("100");
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
    assert!(validate_role(&request, "preview-payload-000").is_ok());
    for role in [
        "preview-payload-001",
        "preview-assemble",
        "preview-signer",
        "",
    ] {
        assert!(validate_role(&request, role).is_err());
    }
}

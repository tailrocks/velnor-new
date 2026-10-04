//! Structural mutations fail before any compiled factory can publish.
use crate::{PermissionLevel, Permissions, WorkflowIr};
use serde_json::{Value, json};

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/native_publish.json")).expect("fixture")
}
fn validate(value: Value) -> bool {
    serde_json::from_value::<WorkflowIr>(value)
        .expect("wire shape")
        .validate()
        .is_ok()
}
#[test]
fn closed_dockerhub_graph_has_narrow_permissions() {
    let workflow: WorkflowIr = serde_json::from_value(fixture()).expect("wire");
    workflow.validate().expect("closed graph");
    assert_eq!(Permissions::default().attestations, PermissionLevel::None);
}
#[test]
fn privilege_and_reference_mutations_fail() {
    for (pointer, payload) in [
        ("/permissions/attestations", json!("write")),
        ("/jobs/attest/native_publish", Value::Null),
        ("/jobs/attest/permissions/contents", json!("write")),
        ("/jobs/attest/environment", json!("arbitrary-environment")),
        ("/jobs/attest/condition", json!("always()")),
        ("/triggers/push_tags", json!(["v*"])),
        ("/triggers/push_branches", json!(["main"])),
        (
            "/jobs/attest/native_publish/subject_name",
            json!("ghcr.io/owner/image"),
        ),
        (
            "/jobs/attest/native_publish/binding/receipt_step",
            json!("admit"),
        ),
    ] {
        let mut workflow = fixture();
        *workflow.pointer_mut(pointer).expect("field") = payload;
        assert!(!validate(workflow), "{pointer}");
    }
}
#[test]
fn ci_source_and_transport_mutations_fail() {
    for (pointer, payload) in [
        ("/jobs/proof/steps/0/env/SOURCE_SHA", json!("foreign")),
        ("/jobs/proof/condition", json!("always()")),
        (
            "/jobs/proof/permissions",
            json!({"contents":"write","actions":"read","id_token":"none","pull_requests":"none"}),
        ),
        (
            "/jobs/image-index/permissions",
            json!({"contents":"read","actions":"read","id_token":"write","pull_requests":"none"}),
        ),
        ("/jobs/image-index/needs", json!([])),
        (
            "/jobs/image-index/outputs/1/value/output",
            json!("artifact-id"),
        ),
        ("/jobs/attest/steps/2/with/push-to-registry", json!("true")),
        (
            "/jobs/attest/steps/2/with/subject-digest",
            json!("sha256:mutable"),
        ),
        (
            "/jobs/attest/steps/2/env",
            json!({"APPLE_SECRET":"${{ secrets.APPLE_SECRET }}"}),
        ),
    ] {
        let mut workflow = fixture();
        if let Some(value) = workflow.pointer_mut(pointer) {
            *value = payload;
        } else {
            workflow["jobs"][pointer.split('/').nth(2).expect("job")]["permissions"] = payload;
        }
        assert!(!validate(workflow), "{pointer}");
    }
}
#[test]
fn arbitrary_steps_and_conditional_receipts_fail() {
    let mut workflow = fixture();
    workflow["jobs"]["attest"]["steps"][1]["condition"] = json!("false");
    assert!(!validate(workflow));
    let mut workflow = fixture();
    workflow["jobs"]["attest"]["steps"][1] =
        json!({"name":"Repository command","kind":"shell","run":["bash","repository.sh"]});
    assert!(!validate(workflow));
}

fn desktop_fixture() -> Value {
    let mut value = fixture();
    let condition = "success() && github.repository == 'owner/project' && startsWith(github.ref, 'refs/tags/v') && github.event_name == 'push'";
    let subject = "dist/App-${{ steps.admit.outputs.version }}-aarch64-apple-darwin.zip";
    let role = &mut value["jobs"]["attest"]["native_publish"];
    role["kind"] = json!("desktop-tag-zip");
    role["environment"] = json!("desktop-release");
    role["subject_path"] = json!(subject);
    role.as_object_mut().expect("role").remove("subject_name");
    let mut invocation = role["binding"]["admission"].clone();
    invocation["helper"]["operation"] = json!("native-publish-admission");
    invocation["helper"]["path"] = json!(".github/velnor/native_publish_admission.sh");
    role["binding"]["admission"] = invocation.clone();
    role["binding"]["full_ci_admission"] = invocation.clone();
    let mut receipt = invocation.clone();
    receipt["helper"]["operation"] = json!("native-publish-receipt-verifier");
    receipt["helper"]["path"] = json!(".github/velnor/native_publish_receipt.sh");
    role["binding"]["receipt"] = receipt.clone();
    role.as_object_mut()
        .expect("role")
        .remove("subject_digest_output");
    let proof_env = json!({"APPROVED_REPOSITORY":"owner/project", "APPROVED_DEFAULT_BRANCH":"main", "APPROVED_SOURCE_SHA":"${{ github.sha }}", "APPROVED_SOURCE_REF":"${{ github.ref }}", "GH_TOKEN":"${{ github.token }}"});
    value["jobs"]["proof"]["steps"][0]["invocation"] = invocation.clone();
    value["jobs"]["proof"]["steps"][0]["env"] = proof_env.clone();
    value["jobs"]["attest"]["steps"][0]["invocation"] = invocation;
    value["jobs"]["attest"]["steps"][1]["invocation"] = receipt;
    let mut admission_env = proof_env;
    admission_env["FULL_CI_RESULT"] = json!("${{ needs.proof.result }}");
    admission_env["EXPECTED_ARTIFACT_ID"] = json!("${{ needs.image-index.outputs.artifact_id }}");
    admission_env["EXPECTED_ARTIFACT_DIGEST"] =
        json!("${{ needs.image-index.outputs.artifact_digest }}");
    value["jobs"]["attest"]["steps"][0]["env"] = admission_env.clone();
    value["jobs"]["attest"]["steps"][1]["env"] = admission_env;

    value["jobs"]["proof"]["outputs"] = json!([]);
    value["jobs"]["proof"]["condition"] = json!(condition);
    value["jobs"]["attest"]["condition"] = json!(condition);
    value["jobs"]["attest"]["environment"] = json!("desktop-release");
    let environment = &mut value["jobs"]["attest"]["steps"][1]["env"];
    environment
        .as_object_mut()
        .expect("env")
        .remove("SUBJECT_NAME");
    environment["SUBJECT_PATH"] = json!(subject);
    value["jobs"]["attest"]["steps"][2]["with"] = json!({"subject-path":subject});
    value
}
#[test]
fn desktop_zip_has_exact_subject_and_push_only_authority() {
    assert!(validate(desktop_fixture()));
    for path in [
        "dist/*.zip",
        "../App-${{ steps.admit.outputs.version }}-aarch64-apple-darwin.zip",
        "App-${{ steps.receipt.outputs.version }}-aarch64-apple-darwin.zip",
    ] {
        let mut value = desktop_fixture();
        value["jobs"]["attest"]["native_publish"]["subject_path"] = json!(path);
        value["jobs"]["attest"]["steps"][1]["env"]["SUBJECT_PATH"] = json!(path);
        value["jobs"]["attest"]["steps"][2]["with"]["subject-path"] = json!(path);
        assert!(!validate(value), "{path}");
    }
    let mut value = desktop_fixture();
    value["jobs"]["attest"]["condition"] = json!("github.event_name == 'workflow_dispatch'");
    assert!(!validate(value));
}

#[test]
fn artifact_producer_oidc_fails_role_check_independently() {
    let mut workflow: WorkflowIr = serde_json::from_value(fixture()).expect("wire");
    workflow
        .jobs
        .get_mut("image-index")
        .expect("producer")
        .permissions = Some(Permissions {
        id_token: PermissionLevel::Write,
        ..Permissions::default()
    });
    let job = workflow.jobs.get("attest").expect("attest");
    assert!(
        job.native_publish
            .as_ref()
            .expect("role")
            .validate(job, &workflow)
            .is_err()
    );
}

#[test]
fn coherent_foreign_subject_and_digest_output_fail() {
    let mut value = fixture();
    let subject = "ghcr.io/owner/image";
    value["jobs"]["attest"]["native_publish"]["subject_name"] = json!(subject);
    value["jobs"]["attest"]["native_publish"]["environment"]["image"] = json!(subject);
    value["jobs"]["attest"]["native_publish"]["environment"]["receipt"]["IMAGE"] = json!(subject);
    value["jobs"]["attest"]["steps"][1]["env"]["IMAGE"] = json!(subject);
    value["jobs"]["attest"]["steps"][2]["with"]["subject-name"] = json!(subject);
    assert!(!validate(value));
    let mut value = fixture();
    value["jobs"]["attest"]["native_publish"]["subject_digest_output"] = json!("foreign_digest");
    value["jobs"]["attest"]["steps"][2]["with"]["subject-digest"] =
        json!("${{ steps.receipt.outputs.foreign_digest }}");
    assert!(!validate(value));
}

#[test]
fn preparation_cannot_receive_disguised_credentials() {
    for credential in [
        "${{ github.token }}",
        "${{secrets.GITHUB_TOKEN}}",
        "${{  secrets.APPLE_SECRET }}",
    ] {
        let mut value = fixture();
        let mut invocation =
            value["jobs"]["attest"]["native_publish"]["binding"]["admission"].clone();
        invocation["helper"]["operation"] = json!("mise-tool-prepare");
        invocation["helper"]["path"] = json!(".github/velnor/mise_tool_prepare.sh");
        let environment = json!({"APPLE_SECRET": credential});
        value["jobs"]["attest"]["native_publish"]["binding"]["preparation"] = json!([{
            "step_id":"prepare", "invocation":invocation.clone(), "environment":environment.clone()
        }]);
        value["jobs"]["attest"]["steps"].as_array_mut().expect("steps").insert(0, json!({
            "id":"prepare", "name":"Prepare", "kind":"source_bound_helper", "invocation":invocation, "env":environment
        }));
        assert!(!validate(value), "{credential}");
    }
}

#[test]
fn actual_oci_phases_and_coherent_source_environments_are_closed() {
    for (field, phase) in [
        ("full_ci_admission", "source"),
        ("admission", "assembly"),
        ("receipt", "admission"),
    ] {
        let mut value = fixture();
        value["jobs"]["attest"]["native_publish"]["binding"][field]["args"][0] = json!(phase);
        match field {
            "full_ci_admission" => {
                value["jobs"]["proof"]["steps"][0]["invocation"]["args"][0] = json!(phase);
            }
            "admission" => {
                value["jobs"]["attest"]["steps"][0]["invocation"]["args"][0] = json!(phase);
            }
            _ => value["jobs"]["attest"]["steps"][1]["invocation"]["args"][0] = json!(phase),
        }
        let workflow: WorkflowIr = serde_json::from_value(value.clone()).expect("wire");
        let job = workflow.jobs.get("attest").expect("attest");
        assert!(
            job.native_publish
                .as_ref()
                .expect("role")
                .validate(job, &workflow)
                .is_err(),
            "{field}"
        );
        assert!(!validate(value), "{field}");
    }
    let mut value = fixture();
    value["jobs"]["attest"]["native_publish"]["environment"]["full_ci"]["SOURCE_SHA"] =
        json!("foreign");
    value["jobs"]["proof"]["steps"][0]["env"]["SOURCE_SHA"] = json!("foreign");
    assert!(!validate(value));
    let mut value = fixture();
    value["jobs"]["attest"]["native_publish"]["environment"]["receipt"]["DOCKER_CONFIG"] =
        json!("${{ runner.temp }}/velnor/oci-docker");
    value["jobs"]["attest"]["steps"][1]["env"]["DOCKER_CONFIG"] =
        json!("${{ runner.temp }}/velnor/oci-docker");
    assert!(!validate(value));
    let mut value = fixture();
    value["jobs"]["proof"]["outputs"] = json!([]);
    assert!(!validate(value));
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn assert_reviewed_rendered_workflow(
    bytes: &str,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = serde_saphyr::from_str(bytes)?;
    for key in ["env", "defaults", "permissions"] {
        let mut tampered = yaml.clone();
        tampered
            .as_object_mut()
            .ok_or("workflow")?
            .insert(key.into(), serde_json::json!({"PATH": "/tmp/forged"}));
        assert!(
            crate::release::crate_release_pr_acquisition::verify_reviewed_workflow(
                serde_json::to_string(&tampered)?.as_bytes(),
                intent
            )
            .is_err(),
            "accepted global {key}"
        );
    }
    for (path, value) in [
        ("/concurrency/group", serde_json::json!("forged")),
        ("/concurrency/cancel-in-progress", serde_json::json!(true)),
        ("/concurrency/queue", serde_json::json!("single")),
        (
            "/jobs/release-preflight/permissions/contents",
            serde_json::json!("write"),
        ),
        (
            "/jobs/release-preparation/outputs/changed",
            serde_json::json!("true"),
        ),
        ("/jobs/release-preparation/if", serde_json::json!("true")),
        (
            "/jobs/release-pr-create/permissions/id-token",
            serde_json::json!("write"),
        ),
        (
            "/jobs/release-pr-create/steps/1/run",
            serde_json::json!("echo forged"),
        ),
    ] {
        let mut tampered = yaml.clone();
        *tampered.pointer_mut(path).ok_or("workflow field")? = value;
        assert!(
            crate::release::crate_release_pr_acquisition::verify_reviewed_workflow(
                serde_json::to_string(&tampered)?.as_bytes(),
                intent
            )
            .is_err(),
            "accepted {path}"
        );
    }
    let mut missing_queue = yaml.clone();
    missing_queue["concurrency"]
        .as_object_mut()
        .ok_or("concurrency")?
        .remove("queue");
    assert!(
        crate::release::crate_release_pr_acquisition::verify_reviewed_workflow(
            serde_json::to_string(&missing_queue)?.as_bytes(),
            intent
        )
        .is_err()
    );
    assert_reviewed_nested_lock(&yaml, intent)?;
    assert_reviewed_preparation_outputs(&yaml)?;
    let publisher = &yaml["jobs"]["release-publish"];
    let event = if yaml["on"].get("workflow_dispatch").is_some() {
        "workflow_dispatch"
    } else {
        "push"
    };
    assert!(
        publisher["if"]
            .as_str()
            .ok_or("gate")?
            .contains(&format!("github.event_name == '{event}'"))
    );
    assert_eq!(publisher["permissions"]["id-token"].as_str(), Some("write"));
    assert_eq!(publisher["environment"].as_str(), Some("release"));
    assert!(yaml["jobs"]["release-publish-bootstrap"].is_null());
    assert!(!bytes.contains("secrets.CARGO_REGISTRY_TOKEN"));
    Ok(())
}

fn assert_reviewed_nested_lock(
    yaml: &serde_json::Value,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut nested_intent = intent.clone();
    nested_intent.pr_write_paths = vec!["nested/workspace/Cargo.lock".into()];
    let mut nested_workflow = yaml.clone();
    nested_workflow["concurrency"]["group"] = serde_json::json!("release-crates-io-owner/project");
    assert!(
        crate::release::crate_release_pr_acquisition::verify_reviewed_workflow(
            serde_json::to_string(&nested_workflow)?.as_bytes(),
            &nested_intent
        )
        .is_ok()
    );
    nested_intent.pr_write_paths.clear();
    assert!(
        crate::release::crate_release_pr_acquisition::verify_reviewed_workflow(
            serde_json::to_string(&nested_workflow)?.as_bytes(),
            &nested_intent
        )
        .is_err()
    );
    nested_intent.pr_write_paths = vec!["Cargo.lock".into(), "nested/workspace/Cargo.lock".into()];
    assert!(
        crate::release::crate_release_pr_acquisition::verify_reviewed_workflow(
            serde_json::to_string(&nested_workflow)?.as_bytes(),
            &nested_intent
        )
        .is_err()
    );
    Ok(())
}

fn assert_reviewed_preparation_outputs(
    yaml: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let prep = &yaml["jobs"]["release-preparation"];
    let steps = prep["steps"].as_array().ok_or("preparation steps")?;
    for id in ["release-pr-candidate", "release-pr-artifact"] {
        assert_eq!(
            steps
                .iter()
                .filter(|step| step["id"].as_str() == Some(id))
                .count(),
            1,
            "missing rendered {id}"
        );
    }
    let preflight_steps = yaml["jobs"]["release-preflight"]["steps"]
        .as_array()
        .ok_or("preflight steps")?;
    assert_eq!(
        preflight_steps
            .iter()
            .filter(|step| step["id"].as_str() == Some("release-receipt-upload"))
            .count(),
        1,
        "missing receipt upload id"
    );
    assert_eq!(
        prep["outputs"]["artifact_id"].as_str(),
        Some("${{ steps.release-pr-artifact.outputs.artifact-id }}")
    );
    assert_eq!(
        prep["outputs"]["content_sha256"].as_str(),
        Some("${{ steps.release-pr-candidate.outputs.content_sha256 }}")
    );
    assert_eq!(
        yaml["jobs"]["release-pr-create"]["needs"][0].as_str(),
        Some("release-preparation")
    );
    assert_eq!(
        prep["outputs"]["artifact_digest"].as_str(),
        Some("${{ steps.release-pr-artifact.outputs.artifact-digest }}")
    );
    assert_eq!(
        prep["outputs"]["changed"].as_str(),
        Some("${{ steps.release-pr-candidate.outputs.changed }}")
    );
    let upload = steps
        .iter()
        .find(|step| step["id"].as_str() == Some("release-pr-artifact"))
        .ok_or("upload")?;
    assert_eq!(
        upload["if"].as_str(),
        Some("steps.release-pr-candidate.outputs.changed == 'true'")
    );
    let receipt = preflight_steps
        .iter()
        .find(|step| step["id"].as_str() == Some("release-receipt-upload"))
        .ok_or("receipt")?;
    assert_eq!(
        receipt["if"].as_str(),
        Some("steps.release-admission.outputs.eligible == 'true'")
    );
    assert_reviewed_writer_outputs(yaml)?;
    Ok(())
}

pub(super) fn fixture_dispatch_inputs(plan: &ReleasePlan) -> Vec<DispatchInput> {
    [
        ("plan", plan.plan_id.as_str()),
        ("source_sha", plan.source_sha.as_str()),
        ("version", "0.1.1"),
    ]
    .into_iter()
    .map(|(name, value)| DispatchInput {
        name: name.to_owned(),
        description: name.to_owned(),
        required: true,
        default: Some(value.to_owned()),
    })
    .collect()
}

fn assert_reviewed_writer_outputs(
    yaml: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let writer = &yaml["jobs"]["release-pr-create"];
    assert_eq!(
        writer["if"].as_str(),
        Some(
            "needs.release-preparation.result == 'success' && needs.release-preparation.outputs.changed == 'true'"
        )
    );
    let writer_steps = writer["steps"].as_array().ok_or("writer steps")?;
    let env = &writer_steps.last().ok_or("writer command")?["env"];
    for (name, output) in [
        ("VELNOR_PR_ARTIFACT_ID", "artifact_id"),
        ("VELNOR_PR_ARTIFACT_DIGEST", "artifact_digest"),
        ("VELNOR_PR_CONTENT_SHA256", "content_sha256"),
    ] {
        assert_eq!(
            env[name].as_str(),
            Some(format!("${{{{ needs.release-preparation.outputs.{output} }}}}").as_str())
        );
    }
    Ok(())
}

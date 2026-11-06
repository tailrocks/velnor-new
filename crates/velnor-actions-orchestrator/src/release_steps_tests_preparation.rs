// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn release_pr_preparation_separates_source_execution_from_write_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let producer = spec.jobs.get("release-preparation").ok_or("producer")?;
    let writer = spec
        .jobs
        .get("release-pr-create")
        .ok_or("missing create-only writer")?;
    assert_eq!(producer.permissions.contents, PermissionLevel::Read);
    assert_eq!(producer.permissions.pull_requests, PermissionLevel::None);
    assert_eq!(producer.permissions.id_token, PermissionLevel::None);
    assert_eq!(writer.permissions.contents, PermissionLevel::Write);
    assert_eq!(writer.permissions.pull_requests, PermissionLevel::Write);
    assert_eq!(writer.permissions.actions, PermissionLevel::Read);
    assert_eq!(writer.permissions.id_token, PermissionLevel::None);
    assert_eq!(writer.needs, ["release-preparation"]);
    assert!(writer.steps.iter().all(|step| !matches!(
        &step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@")
    )));
    assert!(
        producer
            .steps
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::Action { uses, with, .. } if uses.starts_with("actions/checkout@") =>
                    Some(with),
                _ => None,
            })
            .all(|with| with
                .get("persist-credentials")
                .is_some_and(|value| value == "false"))
    );
    let ctx = velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
    };
    let workflow = velnor_actions_workflow_renderer::release_tree::render_release_workflow(
        &spec, &ctx, &rust,
    )?;
    assert!(workflow.contains("release-pr-create:"));
    assert!(workflow.contains("needs.release-preparation.outputs.artifact_id"));
    Ok(())
}

#[test]
fn release_pr_split_rejects_authority_and_artifact_forgery()
-> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    assert!(valid(&spec, &rust));
    let mut candidate = spec.clone();
    candidate
        .jobs
        .get_mut("release-preparation")
        .ok_or("producer")?
        .permissions
        .contents = PermissionLevel::Write;
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    let producer = candidate
        .jobs
        .get_mut("release-preparation")
        .ok_or("producer")?;
    producer.steps[1] = velnor_actions_workflow_renderer::action_step(
        "Checkout",
        crate::workflow::CHECKOUT_USES,
        BTreeMap::from([
            ("persist-credentials".into(), "true".into()),
            ("fetch-depth".into(), "0".into()),
        ]),
    )?;
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    let producer = candidate
        .jobs
        .get_mut("release-preparation")
        .ok_or("producer")?;
    let step = producer
        .steps
        .iter_mut()
        .find(|step| step.name == "Prepare release PR candidate")
        .ok_or("candidate")?;
    let StepKind::Shell { env, .. } = &mut step.kind else {
        return Err("candidate shell".into());
    };
    env.insert("GIT_TOKEN".into(), "${{ secrets.GITHUB_TOKEN }}".into());
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    candidate
        .jobs
        .get_mut("release-preparation")
        .ok_or("producer")?
        .outputs
        .insert(
            "artifact_id".into(),
            "${{ steps.release-pr-artifact.outputs.artifact-digest }}".into(),
        );
    assert!(!valid(&candidate, &rust));
    let mut candidate = spec.clone();
    let producer = candidate
        .jobs
        .get_mut("release-preparation")
        .ok_or("producer")?;
    producer.steps.swap(8, 9);
    assert!(!valid(&candidate, &rust));
    assert_writer_forgery_rejected(&spec, &rust)?;
    Ok(())
}

#[test]
fn disabled_release_pr_keeps_readonly_dry_run_without_writer()
-> Result<(), Box<dyn std::error::Error>> {
    for authentication in [
        ReleaseAuthentication::TrustedPublishing,
        ReleaseAuthentication::BootstrapToken,
    ] {
        let (spec, rust) = fixture_for_release_pr(authentication, false)?;
        assert!(valid(&spec, &rust));
        assert!(!spec.jobs.contains_key("release-pr-create"));
        let producer = spec.jobs.get("release-preparation").ok_or("producer")?;
        assert_eq!(producer.permissions.contents, PermissionLevel::Read);
        assert!(producer.outputs.is_empty());
        assert!(
            producer
                .steps
                .iter()
                .any(|step| step.name == "Validate release")
        );
    }
    Ok(())
}

#[test]
fn rust_artifact_custody_must_precede_publishing() -> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    assert!(
        valid(&spec, &rust),
        "generated release should satisfy custody"
    );
    let mut forged = spec.clone();
    let publisher = forged.jobs.get_mut("release-publish").ok_or("publisher")?;
    let StepKind::Shell { env, .. } = &mut publisher.steps[1].kind else {
        return Err("publisher shell".into());
    };
    env.insert("VELNOR_PREFLIGHT_RUST_ARTIFACT_ID".into(), "forged".into());
    assert!(
        !valid(&forged, &rust),
        "publisher must bind the verified Rust artifact"
    );
    let mut missing_id = spec.clone();
    missing_id
        .jobs
        .get_mut("release-preflight")
        .ok_or("preflight")?
        .outputs
        .remove("rust_artifact_id");
    assert!(
        !valid(&missing_id, &rust),
        "artifact id must be bound to preflight"
    );
    let mut forged_id = spec.clone();
    let upload = forged_id
        .jobs
        .get_mut("release-preflight")
        .ok_or("preflight")?
        .steps
        .iter_mut()
        .find(|step| step.name == "Upload release Rust toolchain")
        .ok_or("upload")?;
    upload.id = Some(StepId::ReleaseAdmission);
    assert!(
        !valid(&forged_id, &rust),
        "artifact upload identity cannot be forged"
    );
    Ok(())
}

#[test]
fn semver_tool_is_required_before_preparation_and_candidate_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    assert!(valid(&spec, &rust));
    for id in ["release-preparation", "release-preflight"] {
        let mut missing = spec.clone();
        let job = missing.jobs.get_mut(id).ok_or("job")?;
        let index = job
            .steps
            .iter()
            .position(|step| step.name == "Acquire cargo-semver-checks")
            .ok_or("semver acquisition")?;
        job.steps.remove(index);
        assert!(!valid(&missing, &rust), "{id} accepted a missing checker");
        let mut changed = spec.clone();
        let job = changed.jobs.get_mut(id).ok_or("job")?;
        let step = job
            .steps
            .iter_mut()
            .find(|step| step.name == "Acquire cargo-semver-checks")
            .ok_or("semver acquisition")?;
        if let velnor_actions_contract::StepKind::Shell { run, .. } = &mut step.kind {
            run[2] = "true".to_owned();
        } else {
            return Err("expected shell step".into());
        }
        assert!(
            !valid(&changed, &rust),
            "{id} accepted an unverified checker"
        );
    }
    Ok(())
}

#[test]
fn consumer_and_reconciliation_depend_on_verified_preflight()
-> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let mut forged_consumer = spec.clone();
    let acquire = forged_consumer
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .steps
        .iter_mut()
        .find(|step| step.name == "Publish approved Rust crate")
        .ok_or("acquire")?;
    let StepKind::Shell { env, .. } = &mut acquire.kind else {
        return Err("acquire is not a shell step".into());
    };
    env.insert(
        "VELNOR_PREFLIGHT_RUST_ARTIFACT_ID".to_owned(),
        "${{ inputs.artifact_id }}".to_owned(),
    );
    assert!(
        !valid(&forged_consumer, &rust),
        "publisher must use the preflight artifact"
    );
    let mut missing_dependency = spec.clone();
    missing_dependency
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .needs
        .clear();
    assert!(
        !valid(&missing_dependency, &rust),
        "publisher must depend on preflight"
    );
    let mut reconcile_without_verification = spec.clone();
    reconcile_without_verification
        .jobs
        .get_mut("release-reconcile")
        .ok_or("reconcile")?
        .steps
        .retain(|step| step.name != "Verify Rust channel manifest");
    assert!(
        !valid(&reconcile_without_verification, &rust),
        "reconciliation must verify its toolchain"
    );
    Ok(())
}

#[test]
fn consumers_acquire_the_receipt_bound_to_preflight() -> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let preflight = spec.jobs.get("release-preflight").ok_or("preflight")?;
    let position = |name: &str| preflight.steps.iter().position(|step| step.name == name);
    assert!(
        position("Release source-admit") < position("Checkout exact source")
            && position("Checkout exact source") < position("Prepare pinned tools")
            && position("Prepare pinned tools") < position("Verify Rust channel manifest")
            && position("Verify Rust channel manifest") < position("Release candidate-receipt")
            && position("Release candidate-receipt") < position("Upload release receipt")
            && position("Upload release receipt") < position("Archive release Rust toolchain"),
        "source admission and Rust verification must precede workspace receipt"
    );
    assert!(
        position("Release verify-package").is_none(),
        "preflight must not verify a package before dependency publication"
    );
    assert_eq!(
        preflight
            .outputs
            .get("receipt_artifact_id")
            .map(String::as_str),
        Some("${{ steps.release-receipt-upload.outputs.artifact-id }}"),
        "preflight must expose the actual receipt upload ID"
    );
    let publisher = spec.jobs.get("release-publish").ok_or("publisher")?;
    assert!(
        publisher
            .needs
            .iter()
            .any(|need| need == "release-preflight")
    );
    let StepKind::Shell { env, .. } = &publisher.steps[1].kind else {
        return Err("publisher shell".into());
    };
    assert_eq!(
        env.get("VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID")
            .map(String::as_str),
        Some("${{ needs.release-preflight.outputs.receipt_artifact_id }}")
    );
    {
        let job = spec.jobs.get("release-reconcile").ok_or("consumer")?;
        assert!(job.needs.iter().any(|need| need == "release-preflight"));
        let operation = "reconciliation-receipt-acquire";
        let acquire = job
            .steps
            .iter()
            .find(|step| step.name == format!("Release {operation}"))
            .ok_or("receipt acquisition")?;
        let StepKind::Shell { run, env } = &acquire.kind else {
            return Err("receipt acquisition is not a shell step".into());
        };
        assert_eq!(run.last().map(String::as_str), Some(operation));
        assert_eq!(
            env.get("VELNOR_RECEIPT_ARTIFACT_ID").map(String::as_str),
            Some("${{ needs.release-preflight.outputs.receipt_artifact_id }}")
        );
        assert!(
            !job.steps
                .iter()
                .any(|step| step.name == "Download release receipt")
        );
    }
    assert!(valid(&spec, &rust));
    assert_consumer_receipt_forgery_rejected(&spec, &rust)?;
    Ok(())
}

fn assert_writer_forgery_rejected(
    spec: &ReleaseWorkflowSpec,
    rust: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut candidate = spec.clone();
    candidate
        .jobs
        .get_mut("release-pr-create")
        .ok_or("writer")?
        .condition = None;
    assert!(!valid(&candidate, rust));
    let mut candidate = spec.clone();
    candidate
        .jobs
        .get_mut("release-pr-create")
        .ok_or("writer")?
        .permissions
        .id_token = PermissionLevel::Write;
    assert!(!valid(&candidate, rust));
    let mut candidate = spec.clone();
    let checkout = candidate
        .jobs
        .get("release-preparation")
        .ok_or("producer")?
        .steps[1]
        .clone();
    candidate
        .jobs
        .get_mut("release-pr-create")
        .ok_or("writer")?
        .steps
        .push(checkout);
    assert!(!valid(&candidate, rust));
    let mut candidate = spec.clone();
    let writer = candidate
        .jobs
        .get_mut("release-pr-create")
        .ok_or("writer")?;
    let StepKind::Shell { env, .. } = &mut writer.steps[1].kind else {
        return Err("writer shell".into());
    };
    env.insert(
        "VELNOR_PR_CONTENT_SHA256".into(),
        "${{ needs.release-preparation.outputs.artifact_digest }}".into(),
    );
    assert!(!valid(&candidate, rust));
    Ok(())
}

fn assert_consumer_receipt_forgery_rejected(
    spec: &ReleaseWorkflowSpec,
    rust: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut forged_id = spec.clone();
    let acquire = forged_id
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .steps
        .iter_mut()
        .find(|step| step.name == "Publish approved Rust crate")
        .ok_or("acquire")?;
    let StepKind::Shell { env, .. } = &mut acquire.kind else {
        return Err("acquire shell".into());
    };
    env.insert(
        "VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID".to_owned(),
        "${{ inputs.artifact_id }}".to_owned(),
    );
    assert!(
        !valid(&forged_id, rust),
        "receipt artifact must come from this preflight"
    );

    let mut altered_upload = spec.clone();
    let upload = altered_upload
        .jobs
        .get_mut("release-preflight")
        .ok_or("preflight")?
        .steps
        .iter_mut()
        .find(|step| step.name == "Upload release receipt")
        .ok_or("upload")?;
    let StepKind::Action { with, .. } = &mut upload.kind else {
        return Err("upload action".into());
    };
    with.insert("name".to_owned(), "unbound-receipt".to_owned());
    assert!(
        !valid(&altered_upload, rust),
        "receipt artifact name must bind run and attempt"
    );

    let mut duplicate = spec.clone();
    let job = duplicate
        .jobs
        .get_mut("release-reconcile")
        .ok_or("reconcile")?;
    let second = job
        .steps
        .iter()
        .find(|step| step.name == "Release reconciliation-receipt-acquire")
        .ok_or("acquire")?
        .clone();
    job.steps.push(second);
    assert!(
        !valid(&duplicate, rust),
        "duplicate receipt acquisition must fail"
    );

    let mut late_upload = spec.clone();
    let job = late_upload
        .jobs
        .get_mut("release-preflight")
        .ok_or("preflight")?;
    let receipt = job
        .steps
        .iter()
        .position(|step| step.name == "Release candidate-receipt")
        .ok_or("receipt")?;
    let verify = job
        .steps
        .iter()
        .position(|step| step.name == "Verify Rust channel manifest")
        .ok_or("verify")?;
    job.steps.swap(receipt, verify);
    assert!(
        !valid(&late_upload, rust),
        "receipt must follow verified Rust"
    );
    Ok(())
}

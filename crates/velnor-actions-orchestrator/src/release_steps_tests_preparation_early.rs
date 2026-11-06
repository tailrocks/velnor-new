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

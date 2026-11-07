// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn crate_release_uses_only_fixed_github_hosted_runner() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_workflow,
    };
    let (spec, rust) = fixture()?;
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
    };
    let workflow = render_release_workflow(&spec, &ctx, &rust)?;
    assert_eq!(
        workflow.matches("runs-on: ubuntu-26.04").count(),
        spec.jobs.len()
    );
    for alternate in ["ubuntu-24.04", "self-hosted", "${{ inputs.runner }}"] {
        let mut altered = spec.clone();
        for job in altered.jobs.values_mut() {
            job.runs_on = alternate.to_owned();
        }
        let mut changed_ctx = ctx.clone();
        changed_ctx.runs_on = alternate.to_owned();
        assert!(render_release_workflow(&altered, &changed_ctx, &rust).is_err());
    }
    Ok(())
}

#[test]
fn rendered_workflow_has_one_hosted_environment_publisher() -> Result<(), Box<dyn std::error::Error>>
{
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_workflow,
    };

    let (spec, rust) = fixture()?;
    let context = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
    };
    let workflow = render_release_workflow(&spec, &context, &rust)?;
    assert!(workflow.contains("release-publish:"));
    assert!(!workflow.contains("release-publish-bootstrap:"));
    assert!(!workflow.contains("secrets.CARGO_REGISTRY_TOKEN"));
    assert!(!workflow.contains("release-plz release"));
    assert!(workflow.contains("Release journal-coordinate"));
    let publisher = spec.jobs.get("release-publish").ok_or("publisher")?;
    assert_eq!(publisher.runs_on, "ubuntu-26.04");
    assert_eq!(publisher.environment.as_deref(), Some("release"));
    assert_eq!(publisher.needs, ["release-preflight"]);
    assert_eq!(publisher.permissions.id_token, PermissionLevel::Write);
    assert_eq!(publisher.permissions.contents, PermissionLevel::Write);
    assert_eq!(
        publisher.condition.as_deref(),
        Some(publish_gate_condition("owner/project", "main", &spec.plan).as_str())
    );
    assert_eq!(publisher.steps[0].name, "Acquire Velnor");
    assert_eq!(
        publisher.steps.last().map(|step| step.name.as_str()),
        Some("Release journal-coordinate")
    );
    assert!(publisher.steps[0].condition.is_none());
    assert!(workflow.contains("id-token: write"));
    assert!(
        workflow.contains("needs: release-preflight") || workflow.contains("- release-preflight")
    );
    assert_publisher_authority_rejected(&spec, &context, &rust)?;
    Ok(())
}

#[test]
fn rendered_bootstrap_has_only_token_publisher() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_workflow,
    };
    let (spec, rust) = fixture_for(ReleaseAuthentication::BootstrapToken)?;
    let context = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    let workflow = render_release_workflow(&spec, &context, &rust)?;
    assert!(workflow.contains("release-publish-bootstrap:"));
    assert!(!workflow.contains("release-publish:"));
    assert!(workflow.contains("secrets.CARGO_REGISTRY_TOKEN"));
    assert!(!workflow.contains("release-plz release"));
    assert!(workflow.contains("Release journal-coordinate"));
    let publisher = spec
        .jobs
        .get("release-publish-bootstrap")
        .ok_or("publisher")?;
    assert_eq!(publisher.environment.as_deref(), Some("release"));
    assert_eq!(publisher.needs, ["release-preflight"]);
    assert!(
        spec.jobs
            .values()
            .all(|job| job.permissions.id_token == PermissionLevel::None)
    );
    for (id, job) in &spec.jobs {
        for step in &job.steps {
            if let StepKind::Shell { env, .. } = &step.kind
                && env
                    .values()
                    .any(|value| value.contains("secrets.CARGO_REGISTRY_TOKEN"))
            {
                assert_eq!(id, "release-publish-bootstrap");
                assert_eq!(step.name, "Release journal-coordinate");
            }
        }
    }
    Ok(())
}

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
    assert!(workflow.contains("Publish approved Rust crate"));
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
    assert_eq!(
        publisher
            .steps
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        ["Capture release budget", "Publish approved Rust crate"]
    );
    assert!(publisher.steps[1].condition.is_none());
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
    assert!(workflow.contains("Publish approved Rust crate"));
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
                assert_eq!(step.name, "Publish approved Rust crate");
            }
        }
    }
    Ok(())
}

#[test]
fn rendered_files_reject_divergent_release_intent() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_files,
    };
    let (spec, rust) = fixture()?;
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    let config = ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".into(),
        semver_check: true,
        packages: vec![ReleasePlzPackage {
            version_group: None,
            name: "example".into(),
            publish_features: vec![],
        }],
    };
    let intent = CrateReleaseIntent {
        schema: 1,
        repository: "owner/project".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        trusted_reviewer_id: 1,
        rust_version: rust,
        generator_version: env!("CARGO_PKG_VERSION").into(),
        generator_source_sha: "b".repeat(40),
        generator_manifest_sha256: "c".repeat(64),
        release_plz_version: "0.4.3".into(),
        runner: "ubuntu-26.04".into(),
        authentication: velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing,
        manifest_path: "Cargo.toml".into(),
        pr_write_paths: vec!["Cargo.toml".into()],
        registry: "crates-io".into(),
        packages: vec!["example".into()],
        publish_features: BTreeMap::from([("example".into(), vec![])]),
        version_groups: std::collections::BTreeMap::new(),
        new_crates: None,
        tag_template: config.tag_pattern.clone(),
        journal_predecessor: None,
        recovery: Some(velnor_actions_contract::config::CrateReleaseRecovery {
            package: "example".into(),
            version: "0.1.1".into(),
            source_sha: "a".repeat(40),
        }),
    };
    assert_release_group_and_authentication_binding(&spec, &ctx, &config, &intent)?;
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    assert_committed_workflow_lock(&files.workflow.bytes, &spec.concurrency.group)?;
    let stale = ".github/release-plz-publish.toml";
    assert!(velnor_actions_workflow_renderer::release_tree::release_stale_paths().contains(&stale));
    assert!(!files.as_sorted_vec().iter().any(|file| file.path == stale));
    assert_eq!(files.as_sorted_vec().len(), 3);
    let generated_paths: std::collections::BTreeSet<_> = files
        .as_sorted_vec()
        .into_iter()
        .map(|file| file.path)
        .collect();
    let planned_paths: std::collections::BTreeSet<_> =
        velnor_actions_workflow_renderer::release_tree::RELEASE_CURRENT_PATHS
            .iter()
            .map(|path| (*path).to_owned())
            .collect();
    assert_eq!(planned_paths, generated_paths);

    assert!(!files.intent.bytes.contains("CARGO_REGISTRY_TOKEN"));
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert_workspace_resume(&spec, &ctx, &config, &intent)?;
    assert_release_features(&spec, &ctx, &config, &intent)?;
    assert_divergent_intent_fields_rejected(&spec, &ctx, &config, &intent)?;
    Ok(())
}

#[test]
fn publisher_budget_is_first_and_cannot_be_forged() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_workflow,
    };
    let (spec, rust) = fixture()?;
    let context = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    let workflow = render_release_workflow(&spec, &context, &rust)?;
    let publisher = spec.jobs.get("release-publish").ok_or("publisher")?;
    assert_eq!(
        publisher.steps.first().map(|step| step.name.as_str()),
        Some("Capture release budget")
    );
    assert!(workflow.contains("steps.release-budget.outputs.budget"));
    assert!(workflow.contains("id: release-budget"));
    let StepKind::Shell { env, .. } = &publisher.steps[0].kind else {
        return Err("budget shell".into());
    };
    assert_eq!(
        env.get("SSL_CERT_FILE").map(String::as_str),
        Some("/etc/ssl/certs/ca-certificates.crt")
    );
    assert_eq!(
        env.get("SSL_CERT_DIR").map(String::as_str),
        Some("/etc/ssl/certs")
    );
    for key in [
        "LD_PRELOAD",
        "LD_AUDIT",
        "LD_LIBRARY_PATH",
        "GLIBC_TUNABLES",
        "BASH_ENV",
        "ENV",
    ] {
        assert_eq!(env.get(key).map(String::as_str), Some(""));
    }
    let mut missing = spec.clone();
    missing
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .steps
        .remove(0);
    assert!(render_release_workflow(&missing, &context, &rust).is_err());
    let mut short_timeout = spec.clone();
    short_timeout
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .timeout_minutes = velnor_actions_contract::JobTimeout::new(1)?;
    assert!(render_release_workflow(&short_timeout, &context, &rust).is_err());
    let mut forged = spec.clone();
    let steps = &mut forged
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .steps;
    let step = steps
        .iter_mut()
        .find(|step| step.name == "Publish approved Rust crate")
        .ok_or("coordinate")?;
    if let StepKind::Shell { env, .. } = &mut step.kind {
        env.insert("VELNOR_RELEASE_BUDGET".into(), "forged".into());
    } else {
        return Err("shell".into());
    }
    assert!(render_release_workflow(&forged, &context, &rust).is_err());
    Ok(())
}

#[test]
fn preflight_artifacts_cover_workflow_lifetime() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_workflow,
    };
    let (spec, rust) = fixture()?;
    let context = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
    };
    let rendered = render_release_workflow(&spec, &context, &rust)?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&rendered)?;
    let steps = workflow["jobs"]["release-preflight"]["steps"]
        .as_array()
        .ok_or("preflight steps")?;
    for id in ["release-receipt-upload", "release-rust-upload"] {
        let step = steps
            .iter()
            .find(|step| step["id"].as_str() == Some(id))
            .ok_or("upload step")?;
        let value = &step["with"]["retention-days"];
        let days = value
            .as_u64()
            .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
            .ok_or("retention days")?;
        assert!(days >= 35, "{id} expires before the full workflow lifetime");
    }
    let verify_index = steps
        .iter()
        .position(|step| step["name"].as_str() == Some("Release verify-preflight-retention"))
        .ok_or("retention verification")?;
    assert_eq!(
        steps[verify_index]["if"].as_str(),
        Some("steps.release-admission.outputs.eligible == 'true'")
    );
    for (id, env_key) in [
        ("release-receipt-upload", "VELNOR_RECEIPT_ARTIFACT_ID"),
        ("release-rust-upload", "VELNOR_RUST_ARTIFACT_ID"),
    ] {
        let index = steps
            .iter()
            .position(|step| step["id"].as_str() == Some(id))
            .ok_or("upload")?;
        assert!(index < verify_index);
        assert_eq!(
            steps[verify_index]["env"][env_key].as_str(),
            Some(format!("${{{{ steps.{id}.outputs.artifact-id }}}}").as_str())
        );
    }
    assert_eq!(
        workflow["jobs"]["release-preflight"]["permissions"]["actions"].as_str(),
        Some("read")
    );
    assert_ne!(
        workflow["jobs"]["release-preflight"]["permissions"]["id-token"].as_str(),
        Some("write")
    );
    Ok(())
}

fn assert_publisher_authority_rejected(
    spec: &ReleaseWorkflowSpec,
    context: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    rust: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::render_release_workflow;
    let mut no_oidc = spec.clone();
    no_oidc
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .permissions
        .id_token = PermissionLevel::None;
    assert!(render_release_workflow(&no_oidc, context, rust).is_err());
    let mut bypass_gate = spec.clone();
    bypass_gate
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .condition = None;
    assert!(render_release_workflow(&bypass_gate, context, rust).is_err());
    let mut two_publishers = spec.clone();
    let mut duplicate = two_publishers
        .jobs
        .get("release-publish")
        .ok_or("publisher")?
        .clone();
    duplicate.role = velnor_actions_workflow_renderer::release_jobs::ReleaseRole::PublishBootstrap;
    two_publishers
        .jobs
        .insert("release-publish-bootstrap".to_owned(), duplicate);
    assert!(render_release_workflow(&two_publishers, context, rust).is_err());
    let mut self_hosted = spec.clone();
    self_hosted
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .runs_on = "self-hosted".to_owned();
    assert!(render_release_workflow(&self_hosted, context, rust).is_err());
    let mut unprotected = spec.clone();
    unprotected
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .environment = None;
    assert!(render_release_workflow(&unprotected, context, rust).is_err());
    let mut no_preflight = spec.clone();
    no_preflight
        .jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .needs
        .clear();
    assert!(render_release_workflow(&no_preflight, context, rust).is_err());
    Ok(())
}

fn assert_divergent_intent_fields_rejected(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    let mut wrong = intent.clone();
    wrong.registry = "alternate".into();
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    let mut wrong = config.clone();
    wrong.tag_pattern = "{{package}}-v{{version}}".into();
    assert!(render_release_files(spec, ctx, &wrong, intent).is_err());
    let mut wrong = intent.clone();
    wrong.recovery.as_mut().ok_or("recovery")?.version = "0.1.2".into();
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    for (key, value) in [
        ("generator", "0.1.99"),
        ("plz", "0.4.4"),
        ("runner", "ubuntu-24.04"),
    ] {
        let mut wrong = intent.clone();
        match key {
            "generator" => wrong.generator_version = value.into(),
            "plz" => wrong.release_plz_version = value.into(),
            _ => wrong.runner = value.into(),
        }
        assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    }
    let mut wrong = intent.clone();
    wrong.repository = "owner/other".into();
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.environment = "other".into();
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.default_branch = "other".into();
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.packages = vec!["other".into()];
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.recovery.as_mut().ok_or("recovery")?.source_sha = "b".repeat(40);
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    let mut wrong = config.clone();
    wrong.packages[0].name = "other".into();
    assert!(render_release_files(spec, ctx, &wrong, intent).is_err());
    Ok(())
}

fn assert_release_group_and_authentication_binding(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    let mut mismatched_group = config.clone();
    mismatched_group.packages[0].version_group = Some("shared".into());
    assert!(render_release_files(spec, ctx, &mismatched_group, intent).is_err());
    let mut grouped_intent = intent.clone();
    grouped_intent
        .version_groups
        .insert("shared".into(), vec!["example".into()]);
    assert!(render_release_files(spec, ctx, config, &grouped_intent).is_err());
    assert!(render_release_files(spec, ctx, &mismatched_group, &grouped_intent).is_ok());
    assert_release_pins(spec, ctx, config, intent);
    assert_predecessor_binding(spec, ctx, config, intent)?;
    let mut wrong_auth = intent.clone();
    wrong_auth.authentication =
        velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    assert!(render_release_files(spec, ctx, config, &wrong_auth).is_err());
    let (bootstrap_spec, _) =
        fixture_for(velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken)?;
    let bootstrap_files = render_release_files(&bootstrap_spec, ctx, config, &wrong_auth)?;
    assert!(
        !bootstrap_files
            .intent
            .bytes
            .contains("CARGO_REGISTRY_TOKEN")
    );
    let bootstrap_decoded: CrateReleaseIntent = toml::from_str(&bootstrap_files.intent.bytes)?;
    assert_eq!(bootstrap_decoded, wrong_auth);
    assert!(render_release_files(&bootstrap_spec, ctx, config, intent).is_err());
    Ok(())
}

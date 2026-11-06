// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[path = "release_steps_tests_publish_early.rs"]
mod early;

#[test]
fn publisher_runs_guarded_durable_coordinator_in_order() -> Result<(), Box<dyn std::error::Error>> {
    for auth in [
        ReleaseAuthentication::TrustedPublishing,
        ReleaseAuthentication::BootstrapToken,
    ] {
        let (spec, rust) = fixture_for(auth)?;
        let job = spec
            .jobs
            .values()
            .find(|job| {
                job.display_name == "Release publish"
                    || job.display_name == "Release bootstrap publish"
            })
            .ok_or("publisher")?;
        assert_eq!(
            job.steps
                .iter()
                .map(|step| step.name.as_str())
                .collect::<Vec<_>>(),
            ["Capture release budget", "Publish approved Rust crate"]
        );
        assert!(valid(&spec, &rust));
        let mut reordered = spec.clone();
        let publisher = reordered
            .jobs
            .values_mut()
            .find(|job| {
                job.display_name == "Release publish"
                    || job.display_name == "Release bootstrap publish"
            })
            .ok_or("publisher")?;
        publisher.steps.swap(0, 1);
        assert!(!valid(&reordered, &rust));
        let mut appended = spec.clone();
        let publisher = appended
            .jobs
            .values_mut()
            .find(|job| {
                job.display_name == "Release publish"
                    || job.display_name == "Release bootstrap publish"
            })
            .ok_or("publisher")?;
        publisher.steps.push(publisher.steps[1].clone());
        assert!(!valid(&appended, &rust));
    }
    Ok(())
}

#[test]
fn workspace_creation_dispatch_binds_source_and_each_package()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::{CrateReleaseIntent, NewCrateRelease};
    use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_files,
    };
    let (mut spec, rust) = fixture_for(ReleaseAuthentication::BootstrapToken)?;
    spec.plan.version = None;
    spec.plan.packages.insert("second".into(), "0.2.0".into());
    spec.triggers
        .dispatch_inputs
        .retain(|input| input.name != "version");
    let creation = NewCrateRelease {
        source_sha: spec.plan.source_sha.clone(),
        packages: spec.plan.packages.clone(),
    };
    spec.new_crates = Some(creation.clone());
    configure_workspace_creation_jobs(&mut spec, &creation)?;
    let config = ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".into(),
        semver_check: true,
        packages: spec
            .plan
            .packages
            .keys()
            .map(|name| ReleasePlzPackage {
                version_group: None,
                name: name.clone(),
                publish_features: vec![],
            })
            .collect(),
    };
    let intent: CrateReleaseIntent = serde_json::from_value(
        serde_json::json!({"schema":1,"repository":"owner/project","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"rust_version":rust,"generator_version":env!("CARGO_PKG_VERSION"),"generator_source_sha":"b".repeat(40),"generator_manifest_sha256":"c".repeat(64),"release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"bootstrap-token","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example","second"],"publish_features":{"example":[],"second":[]},"tag_template":"{{ package }}-v{{ version }}","new_crates":creation,"recovery":null}),
    )?;
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert!(files.workflow.bytes.contains("workflow_dispatch:"));
    assert!(
        files
            .workflow
            .bytes
            .contains("github.event.inputs.source_sha")
    );
    assert!(!files.workflow.bytes.contains("github.event.inputs.version"));
    let yaml: serde_json::Value = serde_saphyr::from_str(&files.workflow.bytes)?;
    let gate = yaml["jobs"]["release-publish-bootstrap"]["if"]
        .as_str()
        .ok_or("publisher gate")?;
    assert!(gate.contains("workflow_dispatch"));
    assert!(!gate.contains("event_name == 'push'"));
    let mut wrong = intent.clone();
    wrong.new_crates.as_mut().ok_or("creation")?.source_sha = "d".repeat(40);
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong
        .new_crates
        .as_mut()
        .ok_or("creation")?
        .packages
        .insert("second".into(), "0.2.1".into());
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    Ok(())
}

#[test]
fn bootstrap_handover_renders_normal_trusted_publisher() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_files,
    };
    let (mut spec, rust) = fixture()?;
    spec.plan.dispatch = false;
    spec.triggers.push_branches = vec!["main".into()];
    spec.plan.version = None;
    spec.plan.packages.insert("second".into(), "0.2.0".into());
    spec.triggers.dispatch_inputs.clear();
    spec.jobs
        .get_mut("release-publish")
        .ok_or("publisher")?
        .condition = Some(publish_gate_condition("owner/project", "main", &spec.plan));
    let config = ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".into(),
        semver_check: true,
        packages: ["example", "second"]
            .into_iter()
            .map(|name| ReleasePlzPackage {
                version_group: None,
                name: name.into(),
                publish_features: vec![],
            })
            .collect(),
    };
    let intent: CrateReleaseIntent = serde_json::from_value(serde_json::json!({
        "schema":1,"repository":"owner/project","default_branch":"main","environment":"release",
        "required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,
        "rust_version":rust,"generator_version":env!("CARGO_PKG_VERSION"),
        "generator_source_sha":"b".repeat(40),"generator_manifest_sha256":"c".repeat(64),
        "release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing",
        "manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example","second"],
        "publish_features":{"example":[],"second":[]},"tag_template":"{{ package }}-v{{ version }}"
    }))?;
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert!(decoded.new_crates.is_none());
    crate::release::crate_release_pr_acquisition::verify_reviewed_workflow(
        files.workflow.bytes.as_bytes(),
        &intent,
    )?;
    crate::release::crate_release_pr_acquisition::integration_fixture::exercise(
        &intent,
        &files.intent.bytes,
        &files.workflow.bytes,
    )?;
    assert_reviewed_rendered_workflow(&files.workflow.bytes, &intent)?;
    let mut mixed_spec = spec.clone();
    mixed_spec.repository = "OWNER/PROJECT".into();
    mixed_spec.plan.repository = "OWNER/PROJECT".into();
    mixed_spec.concurrency =
        velnor_actions_workflow_renderer::release_spec::lock::stable_lock_group(
            "crates-io",
            "OWNER/PROJECT",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        )?;
    for job in mixed_spec.jobs.values_mut() {
        if let Some(condition) = &mut job.condition {
            *condition = condition.replace("owner/project", "OWNER/PROJECT");
        }
    }
    let mut mixed_intent = intent.clone();
    mixed_intent.repository = "OWNER/PROJECT".into();
    let mixed_files = render_release_files(&mixed_spec, &ctx, &config, &mixed_intent)?;
    crate::release::crate_release_pr_acquisition::integration_fixture::exercise(
        &mixed_intent,
        &mixed_files.intent.bytes,
        &mixed_files.workflow.bytes,
    )?;
    Ok(())
}

#[test]
fn supervised_publisher_rejects_pin_budget_environment_and_order_tampering()
-> Result<(), Box<dyn std::error::Error>> {
    for auth in [
        ReleaseAuthentication::TrustedPublishing,
        ReleaseAuthentication::BootstrapToken,
    ] {
        let (spec, rust) = fixture_for(auth)?;
        assert!(valid(&spec, &rust));
        for case in [
            "source",
            "manifest",
            "budget",
            "extra-env",
            "secret",
            "extra-step",
        ] {
            let mut altered = spec.clone();
            let job = altered.jobs.values_mut().find(|job| {
                matches!(job.role, velnor_actions_workflow_renderer::release_jobs::ReleaseRole::PublishOidc
                    | velnor_actions_workflow_renderer::release_jobs::ReleaseRole::PublishBootstrap)
            }).ok_or("publisher")?;
            if case == "extra-step" {
                job.steps.push(job.steps[0].clone());
            } else {
                let StepKind::Shell { run, env } = &mut job.steps[1].kind else {
                    return Err("publisher shell".into());
                };
                match case {
                    "source" => {
                        let i = run.len() - 2;
                        run[i] = "d".repeat(40);
                    }
                    "manifest" => {
                        let i = run.len() - 1;
                        run[i] = "e".repeat(64);
                    }
                    "budget" => {
                        env.insert("VELNOR_RELEASE_BUDGET".into(), "forged".into());
                    }
                    "extra-env" => {
                        env.insert("UNEXPECTED".into(), "value".into());
                    }
                    "secret" => {
                        env.insert("UNEXPECTED".into(), "${{ secrets.OTHER }}".into());
                    }
                    _ => return Err("unknown case".into()),
                }
            }
            assert!(!valid(&altered, &rust), "accepted {case} with {auth:?}");
        }
    }
    Ok(())
}

fn configure_workspace_creation_jobs(
    spec: &mut ReleaseWorkflowSpec,
    creation: &velnor_actions_contract::config::NewCrateRelease,
) -> Result<(), Box<dyn std::error::Error>> {
    let release = RustReleaseConfig {
        enabled: true,
        registry: ReleaseRegistry::CratesIo,
        authentication: Some(ReleaseAuthentication::BootstrapToken),
        packages: vec!["example".into(), "second".into()],
        new_crates: Some(creation.clone()),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        trusted_reviewer_id: 1,
        ..RustReleaseConfig::default()
    };
    let catalog = ToolCatalog::pinned().with_export_rustup_toolchain(false);
    let mise = MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(
        env!("CARGO_PKG_VERSION"),
        "ubuntu-26.04",
        &"b".repeat(40),
        &"c".repeat(64),
    )?;
    let jobs = assemble_jobs(&JobInputs { release: &release, gate: publish_gate_condition(&spec.repository, "main", &spec.plan), preparation_gate: "github.repository == 'owner/project' && github.event_name == 'push' && github.ref == 'refs/heads/main' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'".into(), registry: None, label: "ubuntu-26.04", mise: &mise, catalog: &catalog, acquire: &acquire, generator_version: env!("CARGO_PKG_VERSION"), generator_source_sha: &"b".repeat(40), generator_manifest_sha256: &"c".repeat(64) })?;
    spec.jobs = jobs;
    Ok(())
}

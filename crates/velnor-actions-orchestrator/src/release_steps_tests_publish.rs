// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn release_tool_expectations_require_a_valid_consumer_manifest() -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");
    let raw = consumer_manifest(version);
    let acquire = |json: &str| crate::pins::consumer_acquire_step_with_manifest("ubuntu-26.04", version, Some(json));
    assert!(acquire(&raw).is_ok());
    let mut wrong: serde_json::Value = serde_json::from_str(&raw)?;
    wrong["version"] = "9.9.9".into();
    assert!(acquire(&serde_json::to_string(&wrong)?).is_err());
    let mut missing: serde_json::Value = serde_json::from_str(&raw)?;
    missing["targets"].as_array_mut().ok_or("targets")?.retain(|target| target["target"] != "x86_64-unknown-linux-gnu");
    assert!(acquire(&serde_json::to_string(&missing)?).is_err());
    let mut duplicate: serde_json::Value = serde_json::from_str(&raw)?;
    let targets = duplicate["targets"].as_array_mut().ok_or("targets")?;
    targets.push(targets[0].clone());
    assert!(acquire(&serde_json::to_string(&duplicate)?).is_err());
    let mut malformed: serde_json::Value = serde_json::from_str(&raw)?;
    malformed["targets"][0]["sha256"] = "not-a-digest".into();
    assert!(acquire(&serde_json::to_string(&malformed)?).is_err());
    Ok(())
}

#[test]
fn publishing_jobs_verify_generator_before_running_it() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::StepKind;
    use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;
    use velnor_actions_workflow_renderer::release_tree::{render_release_workflow, ReleaseRenderContext};
    let context = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-26.04".into() };
    for version in ["1..2", "1.2.3-rc.1", "latest"] {
        assert!(velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(version, "ubuntu-26.04", &"b".repeat(40), &"c".repeat(64)).is_err());
    }
    assert!(velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step("1.2.3", "ubuntu-24.04", &"b".repeat(40), &"c".repeat(64)).is_err());
    for authentication in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        let (spec, rust) = fixture_for(authentication)?;
        render_release_workflow(&spec, &context, &rust)?;
        let publisher = spec.jobs.values().find(|job| matches!(job.role, ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap)).ok_or("publisher")?;
        assert_eq!(publisher.steps.len(), 2);
        assert_eq!(publisher.steps[0].name, "Capture release budget");
        assert_eq!(publisher.steps[1].name, "Publish approved Rust crate");
        let mut tampered = spec.clone();
        let publisher = tampered.jobs.values_mut().find(|job| matches!(job.role, ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap)).ok_or("publisher")?;
        let StepKind::Shell { run, .. } = &mut publisher.steps[1].kind else { return Err("publisher shell".into()); };
        let last = run.len() - 1;
        run[last] = "d".repeat(64);
        assert!(render_release_workflow(&tampered, &context, &rust).is_err());
        let preflight = spec.jobs.get("release-preflight").ok_or("preflight")?;
        let acquire = preflight.steps.iter().find(|step| step.name == "Acquire Velnor").ok_or("acquire")?;
        let StepKind::Shell { env, .. } = &acquire.kind else { return Err("acquire shell".into()); };
        assert_eq!(env.get("VELNOR_GENERATOR_VERSION").map(String::as_str), Some(env!("CARGO_PKG_VERSION")));
    }
    Ok(())
}

#[test]
fn rendered_bootstrap_rejects_untrusted_generator_before_staging() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use velnor_actions_workflow_renderer::release_tree::{render_release_workflow, ReleaseRenderContext};
    let (spec, rust) = fixture()?;
    let context = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-26.04".into() };
    let workflow = render_release_workflow(&spec, &context, &rust)?;
    let temp = tempfile::tempdir()?;
    let harness = temp.path().join("bootstrap_harness.py");
    std::fs::write(&harness, include_str!("release_steps_tests/bootstrap_harness.py"))?;
    let mut child = Command::new("python3").arg(harness).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    child.stdin.take().ok_or("missing stdin")?.write_all(workflow.as_bytes())?;
    let output = child.wait_with_output()?;
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    Ok(())
}

#[test]
fn publisher_runs_guarded_durable_coordinator_in_order() -> Result<(), Box<dyn std::error::Error>> {
    for auth in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        let (spec, rust) = fixture_for(auth)?;
        let job = spec.jobs.values().find(|job| job.display_name == "Release publish" || job.display_name == "Release bootstrap publish").ok_or("publisher")?;
        assert_eq!(job.steps.iter().map(|step| step.name.as_str()).collect::<Vec<_>>(), ["Capture release budget", "Publish approved Rust crate"]);
        assert!(valid(&spec, &rust));
        let mut reordered = spec.clone();
        let publisher = reordered.jobs.values_mut().find(|job| job.display_name == "Release publish" || job.display_name == "Release bootstrap publish").ok_or("publisher")?;
        publisher.steps.swap(0, 1);
        assert!(!valid(&reordered, &rust));
        let mut appended = spec.clone();
        let publisher = appended.jobs.values_mut().find(|job| job.display_name == "Release publish" || job.display_name == "Release bootstrap publish").ok_or("publisher")?;
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
    use velnor_actions_workflow_renderer::release_tree::{render_release_files, ReleaseRenderContext};
    let (mut spec, rust) = fixture_for(ReleaseAuthentication::BootstrapToken)?;
    spec.plan.version = None;
    spec.plan.packages.insert("second".into(), "0.2.0".into());
    spec.triggers.dispatch_inputs.retain(|input| input.name != "version");
    let creation = NewCrateRelease { source_sha: spec.plan.source_sha.clone(), packages: spec.plan.packages.clone() };
    spec.new_crates = Some(creation.clone());
    let release = RustReleaseConfig { enabled: true, registry: ReleaseRegistry::CratesIo,
        authentication: Some(ReleaseAuthentication::BootstrapToken), packages: vec!["example".into(), "second".into()],
        new_crates: Some(creation.clone()), required_workflow_path: ".github/workflows/ci.yml".into(), trusted_reviewer_id: 1, ..RustReleaseConfig::default() };
    let catalog = ToolCatalog::pinned().with_export_rustup_toolchain(false);
    let mise = MiseSetup { uses: format!("jdx/mise-action@{}", velnor_actions_actionlint::actions::MISE_ACTION_SHA), version: MISE_VERSION.to_owned(), sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned() };
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(env!("CARGO_PKG_VERSION"), "ubuntu-26.04", &"b".repeat(40), &"c".repeat(64))?;
    spec.jobs = assemble_jobs(&JobInputs { release: &release, gate: publish_gate_condition(&spec.repository, "main", &spec.plan), preparation_gate: "github.repository == 'owner/project' && github.event_name == 'push' && github.ref == 'refs/heads/main' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'".into(), registry: None, label: "ubuntu-26.04", mise: &mise, catalog: &catalog, acquire: &acquire, generator_version: env!("CARGO_PKG_VERSION"), generator_source_sha: &"b".repeat(40), generator_manifest_sha256: &"c".repeat(64) })?;
    let config = ReleasePlzConfig { tag_pattern: "{{ package }}-v{{ version }}".into(), semver_check: true, packages: spec.plan.packages.keys().map(|name| ReleasePlzPackage { version_group: None, name: name.clone(), publish_features: vec![] }).collect() };
    let intent: CrateReleaseIntent = serde_json::from_value(serde_json::json!({"schema":1,"repository":"owner/project","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"rust_version":rust,"generator_version":env!("CARGO_PKG_VERSION"),"generator_source_sha":"b".repeat(40),"generator_manifest_sha256":"c".repeat(64),"release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"bootstrap-token","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example","second"],"publish_features":{"example":[],"second":[]},"tag_template":"{{ package }}-v{{ version }}","new_crates":creation,"recovery":null}))?;
    let ctx = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-26.04".into() };
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert!(files.workflow.bytes.contains("workflow_dispatch:"));
    assert!(files.workflow.bytes.contains("github.event.inputs.source_sha"));
    assert!(!files.workflow.bytes.contains("github.event.inputs.version"));
    let yaml: serde_json::Value = serde_saphyr::from_str(&files.workflow.bytes)?;
    let gate = yaml["jobs"]["release-publish-bootstrap"]["if"].as_str().ok_or("publisher gate")?;
    assert!(gate.contains("workflow_dispatch"));
    assert!(!gate.contains("event_name == 'push'"));
    let mut wrong = intent.clone();
    wrong.new_crates.as_mut().ok_or("creation")?.source_sha = "d".repeat(40);
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    let mut wrong = intent.clone();
    wrong.new_crates.as_mut().ok_or("creation")?.packages.insert("second".into(), "0.2.1".into());
    assert!(render_release_files(&spec, &ctx, &config, &wrong).is_err());
    Ok(())
}

#[test]
fn bootstrap_handover_renders_normal_trusted_publisher() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_config::{ReleasePlzConfig, ReleasePlzPackage};
    use velnor_actions_workflow_renderer::release_tree::{render_release_files, ReleaseRenderContext};
    let (mut spec, rust) = fixture()?;
    spec.plan.dispatch = false;
    spec.triggers.push_branches = vec!["main".into()];
    spec.plan.version = None;
    spec.plan.packages.insert("second".into(), "0.2.0".into());
    spec.triggers.dispatch_inputs.clear();
    spec.jobs.get_mut("release-publish").ok_or("publisher")?.condition =
        Some(publish_gate_condition("owner/project", "main", &spec.plan));
    let config = ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".into(),
        semver_check: true,
        packages: ["example", "second"].into_iter().map(|name| ReleasePlzPackage { version_group: None,
            name: name.into(), publish_features: vec![],
        }).collect(),
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
    let ctx = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on:"ubuntu-26.04".into() };
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert!(decoded.new_crates.is_none());
    crate::crate_release_pr_acquisition::verify_reviewed_workflow(files.workflow.bytes.as_bytes(), &intent)?;
    crate::crate_release_pr_acquisition::integration_fixture::exercise(&intent, &files.intent.bytes, &files.workflow.bytes)?;
    assert_reviewed_rendered_workflow(&files.workflow.bytes, &intent)?;
    let mut mixed_spec = spec.clone();
    mixed_spec.repository = "OWNER/PROJECT".into();
    mixed_spec.plan.repository = "OWNER/PROJECT".into();
    mixed_spec.concurrency = velnor_actions_workflow_renderer::release_spec::lock::stable_lock_group(
        "crates-io", "OWNER/PROJECT", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    )?;
    for job in mixed_spec.jobs.values_mut() {
        if let Some(condition) = &mut job.condition {
            *condition = condition.replace("owner/project", "OWNER/PROJECT");
        }
    }
    let mut mixed_intent = intent.clone();
    mixed_intent.repository = "OWNER/PROJECT".into();
    let mixed_files = render_release_files(&mixed_spec, &ctx, &config, &mixed_intent)?;
    crate::crate_release_pr_acquisition::integration_fixture::exercise(&mixed_intent, &mixed_files.intent.bytes, &mixed_files.workflow.bytes)?;
    Ok(())
}


#[test]
fn supervised_publisher_rejects_pin_budget_environment_and_order_tampering()
-> Result<(), Box<dyn std::error::Error>> {
    for auth in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        let (spec, rust) = fixture_for(auth)?;
        assert!(valid(&spec, &rust));
        for case in ["source", "manifest", "budget", "extra-env", "secret", "extra-step"] {
            let mut altered = spec.clone();
            let job = altered.jobs.values_mut().find(|job| {
                matches!(job.role, velnor_actions_workflow_renderer::release_jobs::ReleaseRole::PublishOidc
                    | velnor_actions_workflow_renderer::release_jobs::ReleaseRole::PublishBootstrap)
            }).ok_or("publisher")?;
            if case == "extra-step" {
                job.steps.push(job.steps[0].clone());
            } else {
                let StepKind::Shell { run, env } = &mut job.steps[1].kind else { return Err("publisher shell".into()); };
                match case {
                    "source" => { let i = run.len() - 2; run[i] = "d".repeat(40); },
                    "manifest" => { let i = run.len() - 1; run[i] = "e".repeat(64); },
                    "budget" => { env.insert("VELNOR_RELEASE_BUDGET".into(), "forged".into()); },
                    "extra-env" => { env.insert("UNEXPECTED".into(), "value".into()); },
                    "secret" => { env.insert("UNEXPECTED".into(), "${{ secrets.OTHER }}".into()); },
                    _ => return Err("unknown case".into()),
                }
            }
            assert!(!valid(&altered, &rust), "accepted {case} with {auth:?}");
        }
    }
    Ok(())
}

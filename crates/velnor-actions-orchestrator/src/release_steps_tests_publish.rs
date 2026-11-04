// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn release_tool_expectations_require_a_valid_consumer_manifest() -> Result<(), Box<dyn std::error::Error>> {
    let version = env!("CARGO_PKG_VERSION");
    let raw = consumer_manifest(version);
    let acquire = |json: &str| crate::pins::consumer_acquire_step_with_manifest("ubuntu-24.04", version, Some(json));
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
    let context = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-24.04".into() };
    for version in ["1..2", "1.2.3-rc.1", "latest"] {
        assert!(velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(version, "ubuntu-24.04", &"b".repeat(40), &"c".repeat(64)).is_err());
    }
    assert!(velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step("1.2.3", "ubuntu-26.04", &"b".repeat(40), &"c".repeat(64)).is_err());
    for authentication in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        let (spec, rust) = fixture_for(authentication)?;
        let workflow = render_release_workflow(&spec, &context, &rust)?;
        let mut tampered = spec.clone();
        let publisher = tampered.jobs.values_mut().find(|job| matches!(job.role, ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap)).ok_or("publisher")?;
        if let StepKind::Shell { env, .. } = &mut publisher.steps[0].kind { env.insert("VELNOR_GENERATOR_VERSION".into(), "0.1.99".into()); }
        assert!(render_release_workflow(&tampered, &context, &rust).is_err());
        for job in spec.jobs.values().filter(|job| matches!(job.role, ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap | ReleaseRole::Preflight)) {
            let acquire = job.steps.first().ok_or("missing acquisition")?;
            let StepKind::Shell { run, env } = &acquire.kind else { return Err("wrong acquisition kind".into()); };
            assert_eq!(env.get("VELNOR_GENERATOR_VERSION").map(String::as_str), Some(env!("CARGO_PKG_VERSION")));
            assert_eq!(env.get("GH_TOKEN").map(String::as_str), Some("${{ github.token }}"));
            assert!(run.iter().any(|part| part.contains("/usr/bin/base64 --decode")));
            assert!(workflow.contains("/usr/bin/base64 --decode"));
        }
    }
    Ok(())
}

#[test]
fn rendered_bootstrap_rejects_untrusted_generator_before_staging() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use velnor_actions_workflow_renderer::release_tree::{render_release_workflow, ReleaseRenderContext};
    let (spec, rust) = fixture()?;
    let context = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-24.04".into() };
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
        let names: Vec<_> = job.steps.iter().map(|step| step.name.as_str()).collect();
        let index = |name| names.iter().position(|candidate| *candidate == name).ok_or("missing step");
        assert!(index("Release source-require")? < index("Acquire cargo-semver-checks")?);
        assert!(index("Acquire cargo-semver-checks")? < index("Release receipt-acquire")?);
        assert!(index("Release verify-admission")? < index("Acquire release-plz")?);
        assert!(index("Acquire release-plz")? < index("Release journal-start")?);
        assert!(index("Release journal-start")? < index("Release journal-coordinate")?);
        assert!(!names.contains(&"Publish release"));
        assert!(!names.contains(&"Upload publication record"));
        let coordinate = &job.steps[index("Release journal-coordinate")?];
        let predicate = coordinate.condition.as_deref().ok_or("condition")?;
        for mode in ["publish", "repair", "mixed", "none"] {
            assert!(predicate.contains(&format!("release_mode == '{mode}'")));
        }
        assert!(valid(&spec, &rust));
        let mut tampered = spec.clone();
        let publisher = tampered.jobs.values_mut().find(|job| job.display_name == "Release publish" || job.display_name == "Release bootstrap publish").ok_or("publisher")?;
        let start = publisher.steps.iter().position(|step| step.name == "Release journal-start").ok_or("start")?;
        let coordinate = publisher.steps.iter().position(|step| step.name == "Release journal-coordinate").ok_or("coordinate")?;
        publisher.steps.swap(start, coordinate);
        assert!(!valid(&tampered, &rust));
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
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(env!("CARGO_PKG_VERSION"), "ubuntu-24.04", &"b".repeat(40), &"c".repeat(64))?;
    spec.jobs = assemble_jobs(&JobInputs { release: &release, gate: publish_gate_condition(&spec.repository, "main", &spec.plan, false), preparation_gate: "github.repository == 'owner/project' && github.event_name == 'push' && github.ref == 'refs/heads/main' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'".into(), registry: None, label: "ubuntu-24.04", mise: &mise, catalog: &catalog, acquire: &acquire, generator_version: env!("CARGO_PKG_VERSION") })?;
    let config = ReleasePlzConfig { tag_pattern: "{{ package }}-v{{ version }}".into(), semver_check: true, packages: spec.plan.packages.keys().map(|name| ReleasePlzPackage { name: name.clone(), publish_features: vec![] }).collect() };
    let intent: CrateReleaseIntent = serde_json::from_value(serde_json::json!({"schema":1,"repository":"owner/project","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,"rust_version":rust,"generator_version":env!("CARGO_PKG_VERSION"),"generator_source_sha":"b".repeat(40),"generator_manifest_sha256":"c".repeat(64),"release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"bootstrap-token","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example","second"],"publish_features":{"example":[],"second":[]},"tag_template":"{{ package }}-v{{ version }}","new_crates":creation,"recovery":null}))?;
    let ctx = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on: "ubuntu-24.04".into() };
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
    spec.plan.version = None;
    spec.plan.packages.insert("second".into(), "0.2.0".into());
    spec.triggers.dispatch_inputs.clear();
    spec.jobs.get_mut("release-publish").ok_or("publisher")?.condition =
        Some(publish_gate_condition("owner/project", "main", &spec.plan, true));
    let config = ReleasePlzConfig {
        tag_pattern: "{{ package }}-v{{ version }}".into(),
        semver_check: true,
        packages: ["example", "second"].into_iter().map(|name| ReleasePlzPackage {
            name: name.into(), publish_features: vec![],
        }).collect(),
    };
    let intent: CrateReleaseIntent = serde_json::from_value(serde_json::json!({
        "schema":1,"repository":"owner/project","default_branch":"main","environment":"release",
        "required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":1,
        "rust_version":rust,"generator_version":env!("CARGO_PKG_VERSION"),
        "generator_source_sha":"b".repeat(40),"generator_manifest_sha256":"c".repeat(64),
        "release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing",
        "manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example","second"],
        "publish_features":{"example":[],"second":[]},"tag_template":"{{ package }}-v{{ version }}"
    }))?;
    let ctx = ReleaseRenderContext { generator_version: env!("CARGO_PKG_VERSION").into(), runs_on:"ubuntu-24.04".into() };
    let files = render_release_files(&spec, &ctx, &config, &intent)?;
    let decoded: CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(decoded, intent);
    assert!(decoded.new_crates.is_none());
    crate::crate_release_pr_acquisition::verify_reviewed_workflow(files.workflow.bytes.as_bytes(), &intent)?;
    crate::crate_release_pr_acquisition::integration_fixture::exercise(&intent, &files.intent.bytes, &files.workflow.bytes)?;
    assert_reviewed_rendered_workflow(&files.workflow.bytes, &intent)?;
    Ok(())
}


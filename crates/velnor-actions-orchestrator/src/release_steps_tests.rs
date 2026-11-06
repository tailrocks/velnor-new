// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::config::{ReleaseAuthentication, ReleaseRegistry, RustReleaseConfig};
use velnor_actions_contract::{ReleaseTarget, StepId, StepKind};
use velnor_actions_mise::{MISE_VERSION, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::release_gates::{ReleaseConfigBinding, check_release_jobs};
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::release_permissions::PermissionLevel;
use velnor_actions_workflow_renderer::release_spec::{
    DispatchInput, ReleasePlan, ReleaseTriggers, lock::stable_lock_group, publish_gate_condition,
};
use velnor_actions_workflow_renderer::{MiseSetup, setup::MISE_BINARY_SHA256_LINUX_X64};

use super::{JobInputs, assemble_jobs};

fn consumer_manifest(generator_version: &str) -> String {
    let commit = "b".repeat(40);
    let digest = "a".repeat(64);
    let targets = ReleaseTarget::ALL.iter().map(|target| {
        let target = target.triple();
        format!("{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{generator_version}/velnor-actions-{generator_version}-{target}\",\"sha256\":\"{digest}\"}}")
    }).collect::<Vec<_>>().join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{generator_version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
    )
}

pub(crate) fn fixture() -> Result<(ReleaseWorkflowSpec, String), Box<dyn std::error::Error>> {
    fixture_for(ReleaseAuthentication::TrustedPublishing)
}

fn fixture_for(authentication: ReleaseAuthentication) -> Result<(ReleaseWorkflowSpec, String), Box<dyn std::error::Error>> {
    fixture_for_release_pr(authentication, true)
}

fn fixture_for_release_pr(authentication: ReleaseAuthentication, release_pr: bool) -> Result<(ReleaseWorkflowSpec, String), Box<dyn std::error::Error>> {
    let generator_version = env!("CARGO_PKG_VERSION");
    let repository = "owner/project";
    let source = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let plan = ReleasePlan {
        plan_id: "release-aaaaaaaaaaaa".to_owned(),
        repository: repository.to_owned(),
        source_sha: source.to_owned(),
        registry: "crates-io".to_owned(),
        packages: BTreeMap::from([("example".to_owned(), "0.1.1".to_owned())]),
        version: Some("0.1.1".to_owned()),
        dispatch: true,
    };
    let release = RustReleaseConfig {
        enabled: true,
        registry: ReleaseRegistry::CratesIo,
        authentication: Some(authentication),
        release_pr,
        packages: vec!["example".to_owned()],
        required_workflow_path: ".github/workflows/ci.yml".to_owned(),
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
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(generator_version, "ubuntu-26.04", &"b".repeat(40), &"c".repeat(64))?;
    let jobs = assemble_jobs(&JobInputs {
        release: &release,
        gate: publish_gate_condition(repository, "main", &plan),
        preparation_gate: format!(
            "github.repository == '{repository}' && github.event_name == 'push' && github.ref == 'refs/heads/main' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'"
        ),
        registry: None,
        label: "ubuntu-26.04",
        mise: &mise,
        catalog: &catalog,
        acquire: &acquire,
        generator_version,
    })?;
    let dispatch_inputs = [
        ("plan", plan.plan_id.as_str()),
        ("source_sha", source),
        ("version", "0.1.1"),
    ]
    .into_iter()
    .map(|(name, value)| DispatchInput {
        name: name.to_owned(),
        description: name.to_owned(),
        required: true,
        default: Some(value.to_owned()),
    })
    .collect();
    Ok((
        ReleaseWorkflowSpec {
            name: "Velnor Release".to_owned(),
            repository: repository.to_owned(),
            default_branch: "main".to_owned(),
            triggers: ReleaseTriggers {
                push_branches: Vec::new(),
                    dispatch_inputs,
            },
            concurrency: stable_lock_group("crates-io", repository, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")?,
            jobs,
            plan,
            publish_environment: "release".to_owned(),
            bootstrap_environment: "release".to_owned(),
            journal_predecessor: None,
            new_crates: None,
            release_plz_version: catalog.version(PinnedTool::ReleasePlz).to_owned(),
        },
        catalog.version(PinnedTool::Rust).to_owned(),
    ))
}

fn valid(spec: &ReleaseWorkflowSpec, rust: &str) -> bool {
    spec.validate().is_ok()
        && check_release_jobs(
            spec,
            &ReleaseConfigBinding {
                generator_version: env!("CARGO_PKG_VERSION"),
                release_plz_version: &spec.release_plz_version,
                rust_version: rust,
            },
        )
        .is_ok()
}

#[path = "release_steps_tests_preparation.rs"]
mod preparation;

#[path = "release_steps_tests_render.rs"]
mod render;

fn assert_committed_workflow_lock(workflow: &str, group: &str) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    std::fs::create_dir_all(temp.path().join(".github/workflows"))?;
    let path = temp.path().join(".github/workflows/release.yml");
    std::fs::write(&path, workflow)?;
    for args in [vec!["init", "-q"], vec!["add", "."], vec!["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "workflow"]] {
        assert!(std::process::Command::new("git").current_dir(temp.path()).args(args).status()?.success());
    }
    let output = std::process::Command::new("git").current_dir(temp.path()).args(["rev-parse", "HEAD"]).output()?;
    assert!(output.status.success());
    let sha = std::str::from_utf8(&output.stdout)?.trim();
    std::fs::write(&path, workflow.replace("cancel-in-progress: false", "cancel-in-progress: true"))?;
    let committed = crate::crate_release_runtime::read_workflow_at(temp.path(), sha, &crate::crate_release_git::ReleaseGit::host_fixture())?;
    crate::crate_release_runtime::validate_workflow_concurrency(&committed, group)?;
    assert!(crate::crate_release_runtime::validate_workflow_concurrency(&std::fs::read(path)?, group).is_err());
    Ok(())
}

fn assert_workspace_resume(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_config::ReleasePlzPackage;
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    let mut normal_spec = spec.clone();
    normal_spec.plan.version = None;
    normal_spec.plan.dispatch = false;
    normal_spec.triggers.push_branches = vec!["main".to_owned()];
    normal_spec.triggers.dispatch_inputs.clear();
    normal_spec.jobs.get_mut("release-publish").ok_or("publisher")?.condition = Some(publish_gate_condition("owner/project", "main", &normal_spec.plan));
    let mut normal_intent = intent.clone();
    normal_intent.recovery = None;
    assert!(render_release_files(&normal_spec, ctx, config, &normal_intent).is_ok());
    let mut workspace_spec = normal_spec.clone();
    workspace_spec.plan.packages.insert("second".into(), "0.2.0".into());
    let mut workspace_intent = normal_intent.clone();
    workspace_intent.packages.push("second".into());
    workspace_intent.publish_features.insert("second".into(), vec![]);
    let mut workspace_config = config.clone();
    workspace_config.packages.push(ReleasePlzPackage { version_group: None, name: "second".into(), publish_features: vec![] });
    assert!(render_release_files(&workspace_spec, ctx, &workspace_config, &workspace_intent).is_ok());
    workspace_spec.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor { run_id: 42, run_attempt: 2, workflow_sha: "d".repeat(40) });
    workspace_intent.journal_predecessor = workspace_spec.journal_predecessor.clone();
    assert!(render_release_files(&workspace_spec, ctx, &workspace_config, &workspace_intent).is_err());

    Ok(())
}

fn assert_predecessor_binding(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    let predecessor = velnor_actions_contract::config::CrateReleasePredecessor { run_id: 42, run_attempt: 1, workflow_sha: "d".repeat(40) };
    let mut resume_spec = spec.clone();
    let mut recovery_intent = intent.clone();
    resume_spec.journal_predecessor = Some(predecessor.clone());
    recovery_intent.journal_predecessor = Some(predecessor);
    let files = render_release_files(&resume_spec, ctx, config, &recovery_intent)?;
    let rendered: velnor_actions_contract::config::CrateReleaseIntent = toml::from_str(&files.intent.bytes)?;
    assert_eq!(rendered.journal_predecessor, recovery_intent.journal_predecessor);
    recovery_intent.journal_predecessor.as_mut().ok_or("predecessor")?.run_attempt += 1;
    assert!(render_release_files(&resume_spec, ctx, config, &recovery_intent).is_err());

    Ok(())
}

fn assert_release_pins(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) {
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    for field in ["commit", "digest"] {
        let mut wrong = intent.clone();
        if field == "commit" { wrong.generator_source_sha = "a".repeat(40); } else { wrong.generator_manifest_sha256 = "d".repeat(64); }
        assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    }
}

fn assert_release_features(
    spec: &ReleaseWorkflowSpec,
    ctx: &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::config::CrateReleaseIntent;
    use velnor_actions_workflow_renderer::release_tree::render_release_files;
    let mut feature_config = config.clone();
    feature_config.packages[0].publish_features = vec!["pty".into()];
    let mut feature_intent = intent.clone();
    feature_intent.publish_features.insert("example".into(), vec!["pty".into()]);
    let feature_files = render_release_files(spec, ctx, &feature_config, &feature_intent)?;
    let decoded_features: CrateReleaseIntent = toml::from_str(&feature_files.intent.bytes)?;
    assert_eq!(decoded_features.publish_features.get("example"), Some(&vec!["pty".into()]));
    assert!(feature_files.config.bytes.contains("publish_features = [\"pty\"]"));
    let mut wrong = intent.clone();
    wrong.publish_features.insert("example".into(), vec!["pty".into()]);
    assert!(render_release_files(spec, ctx, config, &wrong).is_err());
    Ok(())
}

#[path = "release_steps_tests_publish.rs"]
mod publish;

fn assert_reviewed_rendered_workflow(bytes: &str, intent: &velnor_actions_contract::config::CrateReleaseIntent) -> Result<(), Box<dyn std::error::Error>> {
    let yaml: serde_json::Value = serde_saphyr::from_str(bytes)?;
    for key in ["env", "defaults", "permissions"] {
        let mut tampered = yaml.clone();
        tampered.as_object_mut().ok_or("workflow")?.insert(key.into(), serde_json::json!({"PATH": "/tmp/forged"}));
        assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&tampered)?.as_bytes(), intent).is_err(), "accepted global {key}");
    }
    for (path, value) in [
        ("/concurrency/group", serde_json::json!("forged")),
        ("/concurrency/cancel-in-progress", serde_json::json!(true)),
        ("/jobs/release-preflight/permissions/contents", serde_json::json!("write")),
        ("/jobs/release-preparation/outputs/changed", serde_json::json!("true")),
        ("/jobs/release-preparation/if", serde_json::json!("true")),
        ("/jobs/release-pr-create/permissions/id-token", serde_json::json!("write")),
        ("/jobs/release-pr-create/steps/1/run", serde_json::json!("echo forged")),
    ] {
        let mut tampered = yaml.clone();
        *tampered.pointer_mut(path).ok_or("workflow field")? = value;
        assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&tampered)?.as_bytes(), intent).is_err(), "accepted {path}");
    }
    let mut nested_intent = intent.clone();
    nested_intent.pr_write_paths = vec!["nested/workspace/Cargo.lock".into()];
    let mut nested_workflow = yaml.clone();
    nested_workflow["concurrency"]["group"] = serde_json::json!("release-crates-io-owner/project-bed46b222d05fcf033c7a100925229762295940199bb5c2e0aba841333bfa949");
    assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&nested_workflow)?.as_bytes(), &nested_intent).is_ok());
    nested_intent.pr_write_paths.clear();
    assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&nested_workflow)?.as_bytes(), &nested_intent).is_err());
    nested_intent.pr_write_paths = vec!["Cargo.lock".into(), "nested/workspace/Cargo.lock".into()];
    assert!(crate::crate_release_pr_acquisition::verify_reviewed_workflow(serde_json::to_string(&nested_workflow)?.as_bytes(), &nested_intent).is_err());
    let prep = &yaml["jobs"]["release-preparation"];
    let steps = prep["steps"].as_array().ok_or("preparation steps")?;
    for id in ["release-pr-candidate", "release-pr-artifact"] {
        assert_eq!(steps.iter().filter(|step| step["id"].as_str() == Some(id)).count(), 1, "missing rendered {id}");
    }
    let preflight_steps = yaml["jobs"]["release-preflight"]["steps"].as_array().ok_or("preflight steps")?;
    assert_eq!(preflight_steps.iter().filter(|step| step["id"].as_str() == Some("release-receipt-upload")).count(), 1, "missing receipt upload id");
    assert_eq!(prep["outputs"]["artifact_id"].as_str(), Some("${{ steps.release-pr-artifact.outputs.artifact-id }}"));
    assert_eq!(prep["outputs"]["content_sha256"].as_str(), Some("${{ steps.release-pr-candidate.outputs.content_sha256 }}"));
    assert_eq!(yaml["jobs"]["release-pr-create"]["needs"][0].as_str(), Some("release-preparation"));
    assert_eq!(prep["outputs"]["artifact_digest"].as_str(), Some("${{ steps.release-pr-artifact.outputs.artifact-digest }}"));
    assert_eq!(prep["outputs"]["changed"].as_str(), Some("${{ steps.release-pr-candidate.outputs.changed }}"));
    let upload = steps.iter().find(|step| step["id"].as_str() == Some("release-pr-artifact")).ok_or("upload")?;
    assert_eq!(upload["if"].as_str(), Some("steps.release-pr-candidate.outputs.changed == 'true'"));
    let receipt = preflight_steps.iter().find(|step| step["id"].as_str() == Some("release-receipt-upload")).ok_or("receipt")?;
    assert_eq!(receipt["if"].as_str(), Some("steps.release-admission.outputs.eligible == 'true'"));
    let writer = &yaml["jobs"]["release-pr-create"];
    assert_eq!(writer["if"].as_str(), Some("needs.release-preparation.result == 'success' && needs.release-preparation.outputs.changed == 'true'"));
    let writer_steps = writer["steps"].as_array().ok_or("writer steps")?;
    let env = &writer_steps.last().ok_or("writer command")?["env"];
    for (name, output) in [("VELNOR_PR_ARTIFACT_ID", "artifact_id"), ("VELNOR_PR_ARTIFACT_DIGEST", "artifact_digest"), ("VELNOR_PR_CONTENT_SHA256", "content_sha256")] {
        assert_eq!(env[name].as_str(), Some(format!("${{{{ needs.release-preparation.outputs.{output} }}}}").as_str()));
    }
    let publisher = &yaml["jobs"]["release-publish"];
    let event = if yaml["on"].get("workflow_dispatch").is_some() { "workflow_dispatch" } else { "push" };
    assert!(publisher["if"].as_str().ok_or("gate")?.contains(&format!("github.event_name == '{event}'")));
    assert_eq!(publisher["permissions"]["id-token"].as_str(), Some("write"));
    assert_eq!(publisher["environment"].as_str(), Some("release"));
    assert!(yaml["jobs"]["release-publish-bootstrap"].is_null());
    assert!(!bytes.contains("secrets.CARGO_REGISTRY_TOKEN"));
    Ok(())
}

#[path = "release_steps_tests_checkout.rs"]
mod checkout;

#[test]
fn exact_source_dispatch_renders_without_push_trigger() -> Result<(), Box<dyn std::error::Error>> {
    let (mut spec, rust) = fixture()?;
    spec.triggers.push_branches.clear();
    let ctx = velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    let workflow = velnor_actions_workflow_renderer::release_tree::render_release_workflow(&spec, &ctx, &rust)?;
    let yaml: serde_json::Value = serde_saphyr::from_str(&workflow)?;
    assert!(yaml["on"].get("push").is_none());
    assert!(yaml["on"].get("workflow_dispatch").is_some());
    Ok(())
}

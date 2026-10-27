// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod completed_generator_verification;
mod homebrew_adoption_seed_public_assertions;

use std::{error::Error, fs, io, path::Path, process::Command};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use velnor_actions_contract::{GeneratorLock, ReleaseTarget, Step, StepKind, StepRole};
use velnor_actions_orchestrator::{OrchestratorError, finalized_jobs, prepare};
use velnor_actions_workflow_renderer::{
    HomebrewAdoptionSeedSdkIdentity, MiseSetup, ProductReleasePins, render_homebrew_adoption_seed,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HOST_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MANIFEST_PATH: &str = ".velnor/release-manifest.json";

#[test]
fn public_consumer_required_verdict_never_executes_candidate_prelude() -> Result<(), Box<dyn Error>>
{
    let temp = repository()?;
    fs::create_dir_all(temp.path().join(".velnor"))?;
    fs::write(
        temp.path().join(".velnor/config.toml"),
        homebrew_adoption_seed_public_assertions::CONSUMER_CONFIG,
    )?;
    fs::write(
        temp.path().join(MANIFEST_PATH),
        release_manifest(VERSION, SOURCE_SHA, &current_executable_sha256()?),
    )?;
    let action = temp.path().join(".github/actions/velnor-tools-prelude-u26");
    fs::create_dir_all(&action)?;
    fs::write(
        action.join("action.yml"),
        "name: candidate-controlled\nruns:\n  using: composite\n  steps:\n    - shell: bash\n      run: exit 0\n",
    )?;
    let preparation = prepare(temp.path())?;
    homebrew_adoption_seed_public_assertions::assert_raw_consumer_render(&preparation)?;
    homebrew_adoption_seed_public_assertions::assert_tofu_inventory_rejected(&preparation)?;
    homebrew_adoption_seed_public_assertions::assert_native_context_rejections(
        &preparation,
        velnor_actions_contract::WorkflowPolicy::ConsumerV1,
    )?;
    homebrew_adoption_seed_public_assertions::assert_complete_job_authority(
        &preparation,
        velnor_actions_contract::WorkflowPolicy::ConsumerV1,
    )?;
    let jobs = finalized_jobs(&preparation)?;
    homebrew_adoption_seed_public_assertions::assert_finalized_mise_jobs(&jobs)?;
    let required = jobs
        .get(velnor_actions_workflow_renderer::render::FINAL_JOB_ID)
        .ok_or_else(|| io::Error::other("ordinary consumer omitted Required"))?;
    assert_eq!(required.condition.as_deref(), Some("${{ !cancelled() }}"));
    assert!(required.needs.iter().any(|need| need == "plan"));
    assert!(
        jobs.iter()
            .filter(|(id, _)| id.as_str() != velnor_actions_workflow_renderer::render::FINAL_JOB_ID)
            .any(|(_, job)| job
                .steps
                .iter()
                .any(|step| step.role == Some(StepRole::ToolsCacheIdentity)))
    );
    let acquire = required
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::AcquireVelnor))
        .ok_or_else(|| io::Error::other("Required omitted authenticated acquisition"))?;
    assert!(
        required
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::Checkout))
    );
    assert!(required.steps.iter().any(|step| {
        step.role == Some(StepRole::PreparePinnedTools)
            && matches!(&step.kind,
            StepKind::Shell { run, .. } if run.join(" ").contains(&format!("gh@{}", velnor_actions_mise::catalog::GH_VERSION)))
    }));
    homebrew_adoption_seed_public_assertions::assert_trusted_required_steps(
        &required.steps,
        acquire,
    )
}

#[test]
fn public_seed_manifest_replaces_legacy_consumer_manifest() -> Result<(), Box<dyn Error>> {
    let transition = serde_json::from_slice(include_bytes!(
        "../../../.velnor/qualification/homebrew/jackin/transition.json"
    ))?;
    let host_sha = current_executable_sha256()?;
    let manifest = release_manifest(VERSION, SOURCE_SHA, &host_sha);
    let pins = pins();
    let rendered = render_homebrew_adoption_seed(
        &transition,
        &HomebrewAdoptionSeedSdkIdentity {
            source_sha: SOURCE_SHA,
            version: VERSION,
            manifest_bytes: manifest.as_bytes(),
            manifest_sha256: &sha256(manifest.as_bytes()),
            host_target_sha256: &host_sha,
        },
        &pins,
    )?;
    assert_eq!(rendered.len(), 5);
    let rendered_manifest = rendered
        .iter()
        .find(|file| file.path == MANIFEST_PATH)
        .ok_or_else(|| io::Error::other("seed omitted its authenticated release manifest"))?;
    assert_eq!(rendered_manifest.bytes.as_bytes(), manifest.as_bytes());

    let temp = repository()?;
    write_legacy_manifest(temp.path())?;
    apply_rendered(temp.path(), &rendered, false)?;
    let error = prepare(temp.path())
        .err()
        .ok_or_else(|| io::Error::other("legacy manifest unexpectedly prepared"))?;
    assert!(matches!(
        error,
        OrchestratorError::Contract { problem }
            if problem == format!("release_manifest_version_mismatch:0.1.48:{VERSION}")
    ));

    fs::write(
        temp.path().join(MANIFEST_PATH),
        rendered_manifest.bytes.as_bytes(),
    )?;
    let preparation = prepare(temp.path())?;
    homebrew_adoption_seed_public_assertions::assert_raw_consumer_render(&preparation)?;
    Ok(())
}

#[test]
fn public_velnor_repository_bootstrap_accepts_only_catalog_mise_lock() -> Result<(), Box<dyn Error>>
{
    let temp = repository_with_origin(Some("https://github.com/tailrocks/velnor-new.git"))?;
    fs::create_dir_all(temp.path().join(".velnor"))?;
    fs::write(
        temp.path().join(".velnor/config.toml"),
        homebrew_adoption_seed_public_assertions::BOOTSTRAP_CONFIG,
    )?;
    fs::write(
        temp.path().join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    let lock_path = temp.path().join(".velnor/generator.lock");
    let lock_text = include_str!("../../../.velnor/generator.lock");
    fs::write(&lock_path, lock_text)?;
    let published_lock: GeneratorLock =
        velnor_actions_mise::catalog::lock::parse_generator_lock(lock_text)?;
    let preparation = prepare(temp.path())?;
    assert!(
        preparation.lock_audit_blocking.is_empty(),
        "producer preparation must pass lock audit: {:?}",
        preparation.lock_audit_blocking
    );
    let jobs = finalized_jobs(&preparation)?;
    homebrew_adoption_seed_public_assertions::assert_bootstrap_authority(
        &preparation,
        &jobs,
        &published_lock,
    )?;
    homebrew_adoption_seed_public_assertions::assert_raw_producer_render(&preparation)?;
    homebrew_adoption_seed_public_assertions::assert_native_context_rejections(
        &preparation,
        velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1,
    )?;
    homebrew_adoption_seed_public_assertions::assert_complete_job_authority(
        &preparation,
        velnor_actions_contract::WorkflowPolicy::VelnorRepositoryV1,
    )?;
    homebrew_adoption_seed_public_assertions::assert_plan_checkout_pin_is_required(&preparation)?;
    homebrew_adoption_seed_public_assertions::assert_symlink_bootstrap(
        temp.path(),
        &published_lock,
    )?;

    homebrew_adoption_seed_public_assertions::assert_lock_trust_rejections(
        temp.path(),
        &lock_path,
        lock_text,
    )?;
    Ok(())
}

fn apply_rendered(
    root: &Path,
    files: &[velnor_actions_workflow_renderer::RenderedFile],
    manifest: bool,
) -> Result<(), Box<dyn Error>> {
    for file in files {
        if manifest || file.path != MANIFEST_PATH {
            let path = root.join(&file.path);
            fs::create_dir_all(path.parent().ok_or_else(|| {
                io::Error::other(format!("rendered path has no parent: {}", file.path))
            })?)?;
            fs::write(path, file.bytes.as_bytes())?;
        }
    }
    Ok(())
}

fn repository() -> Result<TempDir, Box<dyn Error>> {
    repository_with_origin(None)
}

fn repository_with_origin(origin: Option<&str>) -> Result<TempDir, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let templates = temp.path().join("templates");
    let hooks = temp.path().join("hooks");
    let home = temp.path().join("home");
    let xdg = home.join(".config");
    fs::create_dir_all(&templates)?;
    fs::create_dir_all(&hooks)?;
    fs::create_dir_all(&xdg)?;
    fs::create_dir_all(temp.path().join("src"))?;
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname = \"seed-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(temp.path().join("src/lib.rs"), "pub fn fixture() {}\n")?;
    let git = |args: &[&str]| -> Result<(), Box<dyn Error>> {
        let path =
            std::env::var_os("PATH").ok_or_else(|| io::Error::other("host PATH is unset"))?;
        let output = Command::new("git")
            .args(args)
            .current_dir(temp.path())
            .env_clear()
            .env("PATH", path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &xdg)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
            .env("GIT_TEMPLATE_DIR", &templates)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "Git fixture command failed: {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .into())
        }
    };
    let template_arg = templates
        .to_str()
        .ok_or_else(|| io::Error::other("Git template path is not UTF-8"))?;
    git(&["init", "-b", "main", "--template", template_arg])?;
    if let Some(origin) = origin {
        git(&["remote", "add", "origin", origin])?;
    }
    let hooks_arg = hooks
        .to_str()
        .ok_or_else(|| io::Error::other("Git hooks path is not UTF-8"))?;
    git(&["config", "--local", "core.hooksPath", hooks_arg])?;
    git(&["config", "--local", "commit.gpgsign", "false"])?;
    git(&["config", "user.name", "Fixture"])?;
    git(&["config", "user.email", "fixture@example.invalid"])?;
    git(&["add", "Cargo.toml", "src/lib.rs"])?;
    git(&["commit", "-m", "fixture base"])?;
    Ok(temp)
}

fn write_legacy_manifest(root: &Path) -> Result<(), Box<dyn Error>> {
    let text = release_manifest("0.1.48", SOURCE_SHA, HOST_SHA);
    let path = root.join(MANIFEST_PATH);
    fs::create_dir_all(path.parent().ok_or_else(|| {
        io::Error::other(format!("manifest path has no parent: {}", path.display()))
    })?)?;
    fs::write(path, text)?;
    Ok(())
}

fn release_manifest(version: &str, commit: &str, digest: &str) -> String {
    let targets = [
        ReleaseTarget::LinuxX86_64,
        ReleaseTarget::MacosArm64,
        ReleaseTarget::MacosX86_64,
    ]
    .map(|target| {
        let triple = target.triple();
        let sha = if target == ReleaseTarget::LinuxX86_64 {
            digest
        } else {
            HOST_SHA
        };
        format!(
            "{{\"target\":\"{triple}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{triple}\",\"sha256\":\"{sha}\"}}"
        )
    })
    .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{commit}\",\"targets\":[{targets}]}}"
    )
}

fn pins() -> ProductReleasePins {
    let setup = MiseSetup {
        uses: "jdx/mise-action@f1f6d8a6c2a1f6a79d5216a1d7e3f1b0e4d7c1a9".to_owned(),
        version: "2026.10.19".to_owned(),
        sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned(),
    };
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: vec![],
        install_build_tools_argv: vec![],
        install_qualify_tools_argv: vec![],
        qualify_mbx_action_steps: std::array::from_fn(|_| step()),
        install_runner_build_tools_argv: vec![],
        install_gh_argv: vec!["mise", "install", "gh@2.83.1"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        build_argv: vec![],
        intel_build_argv: vec![],
        install_intel_target_argv: vec![],
        install_resource_probe_target_argv: vec![],
        runner_build_argv: vec![],
        resource_probe_build_argv: vec![],
        install_resource_probe_release_tools_argv: vec![],
        resource_probe_release_build_argv: vec![],
        resource_probe_release_test_argv: vec![],
        lint_argv: vec![],
        verify_lint_argv: vec![],
        gh_argv: vec!["gh".to_owned()],
        rust_version: "1.91.0".to_owned(),
        verify_rust_argv: vec![],
        mr_boxington_version: "1.0.0".to_owned(),
        gh_version: "2.83.1".to_owned(),
    }
}

fn step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: std::collections::BTreeMap::default(),
        },
    }
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn current_executable_sha256() -> Result<String, Box<dyn Error>> {
    let path = std::env::current_exe()?;
    let bytes = fs::read(path)?;
    Ok(sha256(&bytes))
}

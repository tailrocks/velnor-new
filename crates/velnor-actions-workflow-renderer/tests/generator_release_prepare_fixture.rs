// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_workflow_renderer::render::RenderedFile;
use velnor_actions_workflow_renderer::schema2::{
    PRODUCT_RELEASE_WORKFLOW, ProductReleasePins, Schema2WorkflowRequest, render_schema2_workflows,
};
use velnor_actions_workflow_renderer::setup::MiseSetup;

#[path = "generator_release_prepare_fixture/fixture_pins.rs"]
mod fixture_pins;

const SOURCE_SHA: &str = "a97f3c482da31b6f91f83e28c4dca0217e5b6930";
const REPOSITORY: &str = "tailrocks/velnor-new";
const RUST_VERSION: &str = "1.99.1";
const MBX_VERSION: &str = "1.28.0";
const GH_VERSION: &str = "2.83.1";

pub(crate) struct Fixture {
    root: TempDir,
    script: String,
    pub(crate) empty_workspace: PathBuf,
    pub(crate) github_output: PathBuf,
    fake_gh: PathBuf,
    releases: PathBuf,
    tag_ref: PathBuf,
    release_assets: PathBuf,
    scenario: Scenario,
}

#[derive(Clone, Copy)]
pub(crate) enum Scenario {
    Valid,
    CorruptBinary,
    MismatchedManifest,
    WrongProvenanceSource,
    WrongProvenanceToolchain,
    AttestationRefused,
    PartialAttestationRefused,
}

impl Fixture {
    pub(crate) fn new(scenario: Scenario) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let fake_bin = root.path().join("bin");
        let fake_gh = fake_bin.join("gh");
        let empty_workspace = root.path().join("empty-workspace");
        let release_assets = root.path().join("published-assets");
        let github_output = root.path().join("github-output");
        fs::create_dir_all(&fake_bin)?;
        fs::create_dir_all(&empty_workspace)?;
        fs::create_dir_all(&release_assets)?;
        fs::write(&github_output, "")?;
        fs::write(&fake_gh, FAKE_GH)?;
        let mut permissions = fs::metadata(&fake_gh)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_gh, permissions)?;

        let files = render_schema2_workflows(&request(&fake_gh)?)?;
        let workflow = rendered_file(&files, PRODUCT_RELEASE_WORKFLOW)?;
        let (script, asset_names) = extract_prepare_script(workflow)?;

        create_release_assets(&release_assets)?;
        let manifest_assets = release_assets.join("manifest-assets");
        fs::create_dir_all(&manifest_assets)?;
        build_manifest(&release_assets, &manifest_assets)?;
        prepare_asset_bundles(&release_assets, &asset_names)?;
        apply_scenario(&release_assets, scenario)?;

        let releases = root.path().join("releases.json");
        let tag_ref = root.path().join("tag-ref.json");
        write_release_metadata(&releases, &tag_ref, &asset_names)?;

        Ok(Self {
            root,
            script,
            empty_workspace,
            github_output,
            fake_gh,
            releases,
            tag_ref,
            release_assets,
            scenario,
        })
    }

    pub(crate) fn execute(&self) -> Result<Output, Box<dyn Error>> {
        let fake_bin = self
            .fake_gh
            .parent()
            .ok_or("fake GitHub CLI has no parent")?;
        let path = format!("{}:{}", fake_bin.display(), std::env::var("PATH")?);
        let mut command = Command::new("bash");
        command
            .args(["--noprofile", "--norc", "-e", "-o", "pipefail", "-c"])
            .arg(&self.script)
            .current_dir(&self.empty_workspace)
            .env("PATH", path)
            .env("GH_TOKEN", "fixture-token")
            .env("GITHUB_REPOSITORY", REPOSITORY)
            .env("GITHUB_SHA", SOURCE_SHA)
            .env("GITHUB_WORKFLOW_SHA", SOURCE_SHA)
            .env("VELNOR_SOURCE_SHA", SOURCE_SHA)
            .env("VELNOR_WORKFLOW_AUTHORITY_SHA", SOURCE_SHA)
            .env("GITHUB_OUTPUT", &self.github_output)
            .env("GITHUB_STEP_SUMMARY", self.root.path().join("summary"))
            .env("FAKE_GH_RELEASES", &self.releases)
            .env("FAKE_GH_TAG_REF", &self.tag_ref)
            .env("FAKE_RELEASE_ASSETS", &self.release_assets);
        if matches!(self.scenario, Scenario::AttestationRefused) {
            command.env("FAKE_GH_REFUSE_ATTESTATION", "1");
        }
        if matches!(self.scenario, Scenario::PartialAttestationRefused) {
            command.env(
                "FAKE_GH_REFUSE_ATTESTATION_SUBJECT",
                format!(
                    "linux-assets/velnor-actions-{}-x86_64-unknown-linux-gnu",
                    env!("CARGO_PKG_VERSION")
                ),
            );
        }
        Ok(command.output()?)
    }
}

fn request(fake_gh: &Path) -> Result<Schema2WorkflowRequest, Box<dyn Error>> {
    Ok(Schema2WorkflowRequest {
        version: "1".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(pins(fake_gh)),
    })
}

fn pins(fake_gh: &Path) -> ProductReleasePins {
    let setup = MiseSetup {
        uses: "jdx/mise-action@f1f6d8a6c2a1f6a79d5216a1d7e3f1b0e4d7c1a9".to_owned(),
        version: "2026.10.19".to_owned(),
        sha256: "c".repeat(64),
    };
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: fixture_pins::command_argv(),
        install_build_tools_argv: fixture_pins::command_argv(),
        install_qualify_tools_argv: fixture_pins::command_argv(),
        qualify_mbx_action_steps: std::array::from_fn(|_| fixture_pins::fixture_step()),
        install_runner_build_tools_argv: fixture_pins::command_argv(),
        install_gh_argv: fixture_pins::command_argv(),
        build_argv: fixture_pins::command_argv(),
        intel_build_argv: fixture_pins::command_argv(),
        install_intel_target_argv: fixture_pins::command_argv(),
        install_resource_probe_target_argv: fixture_pins::command_argv(),
        runner_build_argv: fixture_pins::command_argv(),
        resource_probe_build_argv: fixture_pins::command_argv(),
        install_resource_probe_release_tools_argv: fixture_pins::command_argv(),
        resource_probe_release_build_argv: fixture_pins::command_argv(),
        resource_probe_release_test_argv: fixture_pins::command_argv(),
        lint_argv: fixture_pins::command_argv(),
        verify_lint_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script(
                GH_VERSION,
                velnor_actions_workflow_renderer::VERIFY_PATH,
            ),
        ],
        gh_argv: vec![fake_gh.display().to_string()],
        rust_version: RUST_VERSION.to_owned(),
        verify_rust_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "{} true",
                velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude()
            ),
        ],
        mr_boxington_version: MBX_VERSION.to_owned(),
        gh_version: GH_VERSION.to_owned(),
    }
}

fn rendered_file<'a>(files: &'a [RenderedFile], path: &str) -> Result<&'a str, Box<dyn Error>> {
    let file = files
        .iter()
        .find(|file| file.path == path)
        .ok_or("generator family workflow is missing")?;
    Ok(&file.bytes)
}

fn extract_prepare_script(workflow: &str) -> Result<(String, Vec<String>), Box<dyn Error>> {
    let step = workflow
        .split("name: Reconcile immutable family release")
        .nth(1)
        .ok_or("generator reconciliation step is missing")?;
    let encoded = step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or("generator reconciliation run command is missing")?;
    let script: String = serde_json::from_str(encoded)?;
    let expected_line = script
        .lines()
        .find(|line| line.starts_with("readonly prepare_expected_assets='"))
        .ok_or("generator release asset list is missing")?;
    let encoded = expected_line
        .strip_prefix("readonly prepare_expected_assets='")
        .and_then(|value| value.strip_suffix('\''))
        .ok_or("generator release asset list is malformed")?;
    let asset_names = serde_json::from_str(encoded)?;
    Ok((script, asset_names))
}

fn create_release_assets(root: &Path) -> Result<(), Box<dyn Error>> {
    for (target, directory) in [
        ("x86_64-unknown-linux-gnu", "linux-assets"),
        ("aarch64-apple-darwin", "macos-assets"),
        ("x86_64-apple-darwin", "macos-intel-assets"),
    ] {
        let asset_name = format!("velnor-actions-{}-{target}", env!("CARGO_PKG_VERSION"));
        let asset_dir = root.join(directory);
        fs::create_dir_all(&asset_dir)?;
        let binary = format!("fixture executable for {target}\n").into_bytes();
        fs::write(asset_dir.join(&asset_name), &binary)?;
        let digest = sha256(&binary);
        fs::write(
            asset_dir.join(format!("{asset_name}.sha256")),
            format!("{digest}  {asset_name}\n"),
        )?;
        let provenance = json!({
            "schema": 1,
            "version": env!("CARGO_PKG_VERSION"),
            "repository": REPOSITORY,
            "commit": SOURCE_SHA,
            "target": target,
            "asset": asset_name,
            "sha256": digest,
            "toolchain": {
                "rust": RUST_VERSION,
                "mr-boxington": MBX_VERSION
            }
        });
        fs::write(
            asset_dir.join(format!("{asset_name}.provenance.json")),
            serde_json::to_vec(&provenance)?,
        )?;
    }
    Ok(())
}

fn build_manifest(root: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/create-release-manifest.sh");
    let result = Command::new("bash")
        .arg(script)
        .args([
            env!("CARGO_PKG_VERSION"),
            REPOSITORY,
            RUST_VERSION,
            MBX_VERSION,
        ])
        .current_dir(root)
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("GITHUB_SHA", SOURCE_SHA)
        .output()?;
    assert!(result.status.success(), "{}", output_text(&result));
    let manifest = root.join("release-manifest.json");
    fs::rename(manifest, output.join("release-manifest.json"))?;
    Ok(())
}

fn prepare_asset_bundles(root: &Path, asset_names: &[String]) -> Result<(), Box<dyn Error>> {
    let directory = root.join("release-attestations");
    fs::create_dir_all(&directory)?;
    for name in asset_names
        .iter()
        .filter(|name| name.ends_with(".intoto.jsonl"))
    {
        fs::write(
            directory.join(name),
            format!("fixture attestation for {name}\n"),
        )?;
    }
    Ok(())
}

fn apply_scenario(root: &Path, scenario: Scenario) -> Result<(), Box<dyn Error>> {
    match scenario {
        Scenario::CorruptBinary => {
            let binary = root.join("linux-assets").join(format!(
                "velnor-actions-{}-x86_64-unknown-linux-gnu",
                env!("CARGO_PKG_VERSION")
            ));
            fs::write(binary, b"corrupt executable\n")?;
        }
        Scenario::MismatchedManifest => {
            let manifest = root.join("manifest-assets/release-manifest.json");
            let mut bytes = fs::read(&manifest)?;
            bytes.push(b' ');
            fs::write(manifest, bytes)?;
        }
        Scenario::WrongProvenanceSource | Scenario::WrongProvenanceToolchain => {
            let provenance = root.join("linux-assets").join(format!(
                "velnor-actions-{}-x86_64-unknown-linux-gnu.provenance.json",
                env!("CARGO_PKG_VERSION")
            ));
            let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&provenance)?)?;
            if matches!(scenario, Scenario::WrongProvenanceSource) {
                record["commit"] = json!("b".repeat(40));
            } else {
                record["toolchain"]["rust"] = json!("1.0.0");
            }
            fs::write(provenance, serde_json::to_vec(&record)?)?;
        }
        _ => {}
    }
    Ok(())
}

fn write_release_metadata(
    releases: &Path,
    tag_ref: &Path,
    asset_names: &[String],
) -> Result<(), Box<dyn Error>> {
    let assets = asset_names
        .iter()
        .map(|name| {
            json!({
                "name": name,
                "state": "uploaded",
                "size": 1,
                "digest": format!("sha256:{}", "1".repeat(64))
            })
        })
        .collect::<Vec<_>>();
    let release = json!([[
        {
            "tag_name": format!("v{}", env!("CARGO_PKG_VERSION")),
            "target_commitish": SOURCE_SHA,
            "draft": false,
            "prerelease": false,
            "immutable": true,
            "assets": assets
        }
    ]]);
    fs::write(releases, serde_json::to_vec(&release)?)?;
    fs::write(
        tag_ref,
        serde_json::to_vec(&json!({"object": {"type": "commit", "sha": SOURCE_SHA}}))?,
    )?;
    Ok(())
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

pub(crate) fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const FAKE_GH: &str = include_str!("generator_release_prepare_fixture/gh.sh");

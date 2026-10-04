//! Generator release for all supported Linux and macOS `velnor-actions` assets.
//!
//! The tag is `generator-<sha>` with `--latest=false`. It does not move
//! `v0.1.0`. Attest jobs never receive `contents: write`. Only publish does.

use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;

use super::Schema2WorkflowRequest;
use super::features::{base, finish};

/// GitHub-hosted macOS label. The arm64 binary is not built on Ubuntu.
const MACOS_RUNS_ON: &str = "macos-15";
/// Intel macOS runner for the x86_64 release binary.
const MACOS_INTEL_RUNS_ON: &str = "macos-15-intel";
#[path = "schema2_generator_release_assets.rs"]
mod assets;
#[path = "schema2_generator_release_manifest.rs"]
mod manifest;
#[path = "schema2_generator_release_workflow_steps.rs"]
mod workflow_steps;

/// Qualify three native builds, attest every product, then publish once.
///
/// # Errors
///
/// An illegal hosted or macOS label fails.
pub(super) fn generator_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let macos = runs_on_yaml(MACOS_RUNS_ON)?;
    let macos_intel = runs_on_yaml(MACOS_INTEL_RUNS_ON)?;
    let mut jobs = vec![assets::source_gate_job(hosted.clone())];
    jobs.extend(linux_jobs(hosted.clone()));
    jobs.extend(macos_arm64_jobs(macos));
    jobs.extend(macos_x86_64_jobs(macos_intel));
    jobs.push(manifest::job(hosted.clone()));
    jobs.push(publish_job(hosted));
    Ok(document(jobs))
}

fn linux_jobs(hosted: Yaml) -> Vec<(String, Yaml)> {
    let steps = assets::build_steps(
        assets::LINUX.binary,
        "Verify ELF architecture",
        &assets::linux_verify(assets::LINUX.binary),
        "sha256sum",
        assets::LINUX.sidecar,
        assets::LINUX.provenance,
        assets::LINUX.target,
        assets::LINUX.archive,
    );
    let upload = [assets::LINUX.archive];
    vec![
        build_job(
            "build-linux",
            "Build Linux velnor-actions",
            hosted.clone(),
            steps,
            "Upload Linux assets",
            assets::LINUX.workflow_artifact,
            &upload,
        ),
        attest_job(
            "attest-linux",
            "Attest Linux velnor-actions",
            hosted,
            "build-linux",
            assets::LINUX,
        ),
    ]
}

fn macos_arm64_jobs(macos: Yaml) -> Vec<(String, Yaml)> {
    let steps = assets::build_steps(
        assets::MACOS_ARM64.binary,
        "Verify Mach-O architecture",
        &assets::macos_verify(assets::MACOS_ARM64.binary, "arm64"),
        "shasum -a 256",
        assets::MACOS_ARM64.sidecar,
        assets::MACOS_ARM64.provenance,
        assets::MACOS_ARM64.target,
        assets::MACOS_ARM64.archive,
    );
    let upload = [assets::MACOS_ARM64.archive];
    vec![
        build_job(
            "build-macos",
            "Build macOS velnor-actions",
            macos.clone(),
            steps,
            "Upload macOS assets",
            assets::MACOS_ARM64.workflow_artifact,
            &upload,
        ),
        attest_job(
            "attest-macos",
            "Attest macOS velnor-actions",
            macos,
            "build-macos",
            assets::MACOS_ARM64,
        ),
    ]
}

fn macos_x86_64_jobs(macos: Yaml) -> Vec<(String, Yaml)> {
    let steps = assets::build_steps(
        assets::MACOS_X86_64.binary,
        "Verify Mach-O x86_64 architecture",
        &assets::macos_verify(assets::MACOS_X86_64.binary, "x86_64"),
        "shasum -a 256",
        assets::MACOS_X86_64.sidecar,
        assets::MACOS_X86_64.provenance,
        assets::MACOS_X86_64.target,
        assets::MACOS_X86_64.archive,
    );
    let upload = [assets::MACOS_X86_64.archive];
    vec![
        build_job(
            "build-macos-intel",
            "Build macOS x86_64 velnor-actions",
            macos.clone(),
            steps,
            "Upload macOS x86_64 assets",
            assets::MACOS_X86_64.workflow_artifact,
            &upload,
        ),
        attest_job(
            "attest-macos-intel",
            "Attest macOS x86_64 velnor-actions",
            macos,
            "build-macos-intel",
            assets::MACOS_X86_64,
        ),
    ]
}

fn publish_script() -> String {
    let verify = manifest::publication_verify_script();
    let assets = manifest::release_asset_paths();
    format!(
        "{verify}\ntag=\"generator-${{GITHUB_SHA}}\"\ngh release create \"$tag\" -R \"${{GITHUB_REPOSITORY}}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"velnor-actions 0.1.0 built from ${{GITHUB_SHA}}.\" {assets}"
    )
}

fn build_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    steps: Vec<Yaml>,
    upload_name: &str,
    artifact: &str,
    files: &[&str],
) -> (String, Yaml) {
    let mut prefixed = vec![workflow_steps::checkout_step()];
    prefixed.extend(steps);
    prefixed.push(workflow_steps::upload_step(upload_name, artifact, files));
    finish(
        id,
        workflow_steps::with_needs(
            workflow_steps::with_permissions(
                base(name, runs_on, 120),
                workflow_steps::build_permissions(),
            ),
            &["verify-release-source"],
        ),
        prefixed,
    )
}

fn attest_job(
    id: &str,
    name: &str,
    runs_on: Yaml,
    needs: &str,
    product: assets::ProductAsset,
) -> (String, Yaml) {
    let qualify = assets::qualification_script(product.binary, product.directory);
    let files = [product.binary, product.sidecar, product.provenance];
    let downloads = assets::download_steps(product, "Download built asset archive");
    let mut steps = vec![
        workflow_steps::checkout_step(),
        workflow_steps::mise_step(),
        workflow_steps::bash_step(
            "Install pinned candidate qualification tools",
            assets::QUALIFICATION_TOOLS_INSTALL,
        ),
    ];
    steps.extend(downloads);
    steps.extend([
        workflow_steps::bash_step(
            "Verify downloaded checksum sidecar",
            &format!(
                "set -eu\ncd {}\n{} {}",
                product.directory, product.checksum_command, product.sidecar
            ),
        ),
        workflow_steps::bash_step(
            "Verify candidate provenance record",
            &assets::verify_provenance_script(product),
        ),
        workflow_steps::bash_step("Qualify downloaded candidate", &qualify),
        workflow_steps::attest_step(&workflow_steps::subject_list(&files, product.directory)),
    ]);
    finish(
        id,
        workflow_steps::with_needs(
            workflow_steps::with_permissions(
                base(name, runs_on, 20),
                workflow_steps::attest_permissions(),
            ),
            &[needs],
        ),
        steps,
    )
}

fn publish_job(hosted: Yaml) -> (String, Yaml) {
    let mut steps = vec![workflow_steps::checkout_step(), workflow_steps::mise_step()];
    steps.push(workflow_steps::bash_step(
        "Install pinned GitHub CLI",
        "mise --no-config --no-env --no-hooks install gh@2.102.0",
    ));
    steps.extend(
        assets::ASSETS
            .iter()
            .flat_map(|asset| assets::download_steps(*asset, "Download release asset archive")),
    );
    steps.push(workflow_steps::download_step(
        "Download release manifest",
        manifest::ARTIFACT,
        manifest::DIR,
    ));
    steps.push(workflow_steps::publish_step(&publish_script()));
    finish(
        "publish-generator",
        workflow_steps::with_needs(
            workflow_steps::with_permissions(
                base("Publish velnor-actions", hosted, 30),
                workflow_steps::publish_permissions(),
            ),
            &[
                "attest-linux",
                "attest-macos",
                "attest-macos-intel",
                "attest-manifest",
            ],
        ),
        steps,
    )
}

fn document(jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Generator release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}

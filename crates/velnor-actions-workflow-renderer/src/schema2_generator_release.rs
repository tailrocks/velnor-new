//! Generator release for all supported Linux and macOS `velnor-actions` assets.
//!
//! The next immutable release is `v0.1.4`. Attest jobs never receive
//! `contents: write`. Only publish does.

use super::{ProductReleasePins, Schema2WorkflowRequest};
use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;
use velnor_actions_contract::ReleaseTarget;

/// GitHub-hosted macOS label. The arm64 binary is not built on Ubuntu.
const MACOS_RUNS_ON: &str = "macos-15";
/// Intel macOS runner for native `x86_64` execution (qualify and attest).
/// The `x86_64` binary itself cross-compiles on the ARM runner, where the
/// pinned `mr-boxington` release is installable.
const MACOS_INTEL_RUNS_ON: &str = "macos-15-intel";
#[path = "schema2_generator_release_archive.rs"]
mod archive;
#[path = "schema2_generator_release_assets.rs"]
mod assets;
#[path = "schema2_generator_release_candidate_manifest.rs"]
mod candidate_manifest;
#[path = "schema2_generator_release_jobs.rs"]
mod jobs;
#[path = "schema2_generator_release_manifest.rs"]
mod manifest;
#[path = "schema2_generator_release_qualification.rs"]
mod qualification;
#[path = "schema2_generator_release_source.rs"]
mod source;
#[path = "schema2_generator_release_workflow_steps.rs"]
mod workflow_steps;

/// The generator-release workflow and its checked-in local composite actions.
pub(super) struct GeneratorRelease {
    pub workflow: Yaml,
    pub actions: Vec<(String, Yaml)>,
}

/// Typed role for one node in the generator release graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobRole {
    SourceGate,
    Build(ReleaseTarget),
    CandidateManifest,
    Qualify(ReleaseTarget),
    Attest(ReleaseTarget),
    AttestManifest,
    Publish,
}

/// Resolve a job ID through the explicit asset/target inventory.
pub(super) fn job_role(id: &str) -> Option<JobRole> {
    match id {
        "verify-release-source" => return Some(JobRole::SourceGate),
        "candidate-manifest" => return Some(JobRole::CandidateManifest),
        "attest-manifest" => return Some(JobRole::AttestManifest),
        "publish-generator" => return Some(JobRole::Publish),
        _ => {}
    }
    assets::ASSETS.iter().find_map(|asset| {
        if id == asset.build_job {
            Some(JobRole::Build(asset.target))
        } else if id == asset.qualify_job {
            Some(JobRole::Qualify(asset.target))
        } else if id == asset.attest_job {
            Some(JobRole::Attest(asset.target))
        } else {
            None
        }
    })
}

/// Canonical paths published by the same-run release-manifest producer.
pub(super) fn publication_asset_paths() -> Vec<String> {
    manifest::release_asset_path_list()
}

/// Version bound into the canonical release manifest and immutable tag.
pub(super) const fn release_version() -> &'static str {
    assets::VERSION
}

/// Validate the candidate manifest against downloaded bytes and provenance.
pub(super) fn verify_published_manifest_script(pins: &ProductReleasePins) -> String {
    manifest::verify_published_manifest_script(pins)
}

/// Verify all downloaded attestation bundles against the exact workflow authority.
pub(super) fn verify_attestation_bundles_script() -> String {
    manifest::attestation_bundle_script()
}

/// Resolve the product-wide pinned Mise setup and GitHub CLI through adapters.
pub(super) fn product_setup_steps(
    pins: &ProductReleasePins,
    target: ReleaseTarget,
) -> Result<Vec<Yaml>, RenderError> {
    Ok(vec![
        mise_setup_step(pins, target)?,
        workflow_steps::install_gh_step(&pins.install_gh_argv)?,
    ])
}

/// Render one verified target's pinned Mise setup action.
pub(super) fn mise_setup_step(
    pins: &ProductReleasePins,
    target: ReleaseTarget,
) -> Result<Yaml, RenderError> {
    workflow_steps::mise_step(pins.setup_for(target))
}

/// Render the pinned GitHub CLI invocation as a shell function.
pub(super) fn gh_function(pins: &ProductReleasePins) -> Result<String, RenderError> {
    workflow_steps::gh_function(&pins.gh_argv)
}

pub(super) fn release_gate_steps(pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    Ok(vec![
        workflow_steps::command_step(
            "Install pinned release gate tools",
            &pins.install_gate_tools_argv,
        )?,
        workflow_steps::command_step("Run actionlint", &pins.actionlint_argv)?,
        workflow_steps::command_step("Run zizmor", &pins.zizmor_argv)?,
        workflow_steps::bash_step("Check release freshness", "bash scripts/check-freshness.sh"),
    ])
}

/// Qualify each native build in isolation, attest every product, then publish once.
///
/// # Errors
///
/// An illegal hosted or macOS label fails.
pub(super) fn generator_release(
    request: &Schema2WorkflowRequest,
) -> Result<GeneratorRelease, RenderError> {
    let pins = request
        .product_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("product_release_pins_missing".to_owned()))?;
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let macos = runs_on_yaml(MACOS_RUNS_ON)?;
    let macos_intel = runs_on_yaml(MACOS_INTEL_RUNS_ON)?;
    let mut actions = Vec::new();
    let source_step = source::qualification_step();
    let mut jobs = vec![assets::source_gate_job(hosted.clone(), pins)?];
    jobs.extend(linux_jobs(
        hosted.clone(),
        pins,
        &source_step,
        &mut actions,
    )?);
    jobs.extend(macos_arm64_jobs(
        macos.clone(),
        pins,
        &source_step,
        &mut actions,
    )?);
    jobs.extend(macos_x86_64_jobs(
        macos,
        macos_intel,
        pins,
        &source_step,
        &mut actions,
    )?);
    jobs.push(candidate_manifest::job(hosted.clone(), pins)?);
    jobs.push(manifest::job(hosted.clone(), pins, &mut actions)?);
    jobs.push(jobs::publish_job(hosted, pins, &mut actions)?);
    Ok(GeneratorRelease {
        workflow: document(jobs),
        actions,
    })
}

fn linux_jobs(
    hosted: Yaml,
    pins: &ProductReleasePins,
    source_step: &Yaml,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    let steps = assets::build_steps(
        assets::LINUX,
        ReleaseTarget::LinuxX86_64,
        "Verify ELF architecture",
        &assets::linux_verify(assets::LINUX.binary),
        pins,
    )?;
    Ok(vec![
        jobs::build_job(
            "build-linux",
            "Build Linux velnor-actions",
            "generator-release-build-linux",
            hosted.clone(),
            steps,
            assets::LINUX,
            actions,
        )?,
        qualification::job(
            qualification::QualificationJob {
                id: "qualify-linux",
                name: "Qualify Linux velnor-actions",
                action: "generator-release-qualify-linux",
                runs_on: hosted.clone(),
                build_job: "build-linux",
                product: assets::LINUX,
                source_step,
            },
            pins,
            actions,
        )?,
        jobs::attest_job(
            "attest-linux",
            "Attest Linux velnor-actions",
            "generator-release-attest-linux",
            hosted,
            assets::LINUX,
            pins,
            actions,
        )?,
    ])
}

fn macos_arm64_jobs(
    macos: Yaml,
    pins: &ProductReleasePins,
    source_step: &Yaml,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    let steps = assets::build_steps(
        assets::MACOS_ARM64,
        ReleaseTarget::MacosArm64,
        "Verify Mach-O architecture",
        &assets::macos_verify(assets::MACOS_ARM64.binary, "arm64"),
        pins,
    )?;
    Ok(vec![
        jobs::build_job(
            "build-macos",
            "Build macOS velnor-actions",
            "generator-release-build-macos",
            macos.clone(),
            steps,
            assets::MACOS_ARM64,
            actions,
        )?,
        qualification::job(
            qualification::QualificationJob {
                id: "qualify-macos",
                name: "Qualify macOS velnor-actions",
                action: "generator-release-qualify-macos",
                runs_on: macos.clone(),
                build_job: "build-macos",
                product: assets::MACOS_ARM64,
                source_step,
            },
            pins,
            actions,
        )?,
        jobs::attest_job(
            "attest-macos",
            "Attest macOS velnor-actions",
            "generator-release-attest-macos",
            macos,
            assets::MACOS_ARM64,
            pins,
            actions,
        )?,
    ])
}

fn macos_x86_64_jobs(
    build_runs_on: Yaml,
    native_runs_on: Yaml,
    pins: &ProductReleasePins,
    source_step: &Yaml,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    let steps = assets::build_steps(
        assets::MACOS_X86_64,
        ReleaseTarget::MacosArm64,
        "Verify Mach-O x86_64 architecture",
        &assets::macos_verify(assets::MACOS_X86_64.binary, "x86_64"),
        pins,
    )?;
    Ok(vec![
        jobs::build_job(
            "build-macos-intel",
            "Build macOS x86_64 velnor-actions",
            "generator-release-build-macos-intel",
            build_runs_on,
            steps,
            assets::MACOS_X86_64,
            actions,
        )?,
        qualification::job(
            qualification::QualificationJob {
                id: "qualify-macos-intel",
                name: "Qualify macOS x86_64 velnor-actions",
                action: "generator-release-qualify-macos-intel",
                runs_on: native_runs_on.clone(),
                build_job: "build-macos-intel",
                product: assets::MACOS_X86_64,
                source_step,
            },
            pins,
            actions,
        )?,
        jobs::attest_job(
            "attest-macos-intel",
            "Attest macOS x86_64 velnor-actions",
            "generator-release-attest-macos-intel",
            native_runs_on,
            assets::MACOS_X86_64,
            pins,
            actions,
        )?,
    ])
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
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("generator-release-${{ github.repository }}-${{ github.ref }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}

#[cfg(test)]
#[path = "schema2_generator_release_tests.rs"]
mod tests;

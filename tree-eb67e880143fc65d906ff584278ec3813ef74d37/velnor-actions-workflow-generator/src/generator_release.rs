//! Generator release for all supported Linux and macOS `velnor-actions` assets.
//!
//! The next immutable release is `v0.1.1`. Attest jobs never receive
//! `contents: write`. Only publish does.

use crate::{generator_release_pins::GeneratorReleasePins, request::Schema2WorkflowRequest};
use velnor_actions_contract_release::ReleaseTarget;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::runs_on::runs_on_yaml;
use velnor_actions_workflow_tree::yaml::Yaml;

/// GitHub-hosted macOS label. The arm64 binary is not built on Ubuntu.
const MACOS_RUNS_ON: &str = "macos-15";
/// Intel macOS runner for the `x86_64` release binary.
const MACOS_INTEL_RUNS_ON: &str = "macos-15-intel";
mod archive;
mod assets;
mod candidate_manifest;
mod jobs;
mod manifest;
mod qualification;
mod source;
mod workflow_steps;

pub use source::QUALIFICATION_SOURCE_PREPARE;

/// The generator-release workflow and its checked-in local composite actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorRelease {
    /// Rendered generator-release workflow body.
    pub workflow: Yaml,
    /// Checked-in local composite actions as `(path, body)` pairs.
    pub actions: Vec<(String, Yaml)>,
}

/// Qualify each native build in isolation, attest every product, then publish once.
///
/// # Errors
///
/// An illegal hosted or macOS label fails.
pub fn generator_release(
    request: &Schema2WorkflowRequest,
) -> Result<GeneratorRelease, RenderError> {
    let pins = request
        .generator_release
        .as_ref()
        .ok_or_else(|| RenderError::InvalidWorkflow("generator_release_pins_missing".to_owned()))?;
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
    pins: &GeneratorReleasePins,
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
    pins: &GeneratorReleasePins,
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
    build_host: Yaml,
    native_qualifier: Yaml,
    pins: &GeneratorReleasePins,
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
            build_host,
            steps,
            assets::MACOS_X86_64,
            actions,
        )?,
        qualification::job(
            qualification::QualificationJob {
                id: "qualify-macos-intel",
                name: "Qualify macOS x86_64 velnor-actions",
                action: "generator-release-qualify-macos-intel",
                runs_on: native_qualifier.clone(),
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
            native_qualifier,
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
mod tests;

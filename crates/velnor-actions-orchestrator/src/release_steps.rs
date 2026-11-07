// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use velnor_actions_contract::config::{ReleaseAuthentication, RustReleaseConfig};
use velnor_actions_contract::{JobTimeout, Step, StepId, StepRole};
use velnor_actions_mise::catalog::release_plz::ReleaseRequest as PlzRelease;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog, ToolHomes};
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseRole};
use velnor_actions_workflow_renderer::release_permissions::JobPermissions;
use velnor_actions_workflow_renderer::release_tree::RELEASE_CONFIG_PATH;
use velnor_actions_workflow_renderer::{
    MiseSetup, action_step, ambient_shell_step, mise_setup_step, steps::UPLOAD_ARTIFACT_USES,
};

use crate::OrchestratorError;
use crate::release::release_checkouts::forge_env;
use crate::utf8::{strings_of, strings_of_env};
use velnor_actions_workflow_renderer::release_checkout_gates::{policy_checkout, source_checkout};

pub(crate) enum Phase {
    Publish,
    DryRun,
}

pub(crate) struct JobInputs<'a> {
    pub(crate) release: &'a RustReleaseConfig,
    pub(crate) gate: String,
    pub(crate) preparation_gate: String,
    pub(crate) registry: Option<&'a str>,
    pub(crate) label: &'a str,
    pub(crate) mise: &'a MiseSetup,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) acquire: &'a Step,
    pub(crate) generator_version: &'a str,
    pub(crate) generator_source_sha: &'a str,
    pub(crate) generator_manifest_sha256: &'a str,
}

struct JobParts<'a> {
    policy: Step,
    source: Step,
    setup: Step,
    prepare_rust: Step,
    verify_rust: Step,
    tool_env: BTreeMap<String, String>,
    executable_env: BTreeMap<String, String>,
    release_plz: Step,
    semver: Step,
    label: &'a str,
    gate: &'a str,
    preparation_gate: &'a str,
    environment: &'a str,
    acquire: Step,
    generator_version: &'a str,
    rust_version: &'a str,
}

pub(crate) fn assemble_jobs(
    inputs: &JobInputs<'_>,
) -> Result<BTreeMap<String, ReleaseJobSpec>, OrchestratorError> {
    if *inputs.acquire
        != velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(
            inputs.generator_version,
            inputs.label,
            inputs.generator_source_sha,
            inputs.generator_manifest_sha256,
        )?
    {
        return Err(OrchestratorError::Contract {
            problem: "release_generator_acquire_mismatch".to_owned(),
        });
    }
    let parts = JobParts {
        policy: policy_checkout(inputs.generator_version)?,
        source: source_checkout(
            inputs.generator_version,
            "${{ steps.release-admission.outputs.source_sha }}",
        )?,
        setup: mise_setup_step(inputs.mise)?,
        prepare_rust: velnor_actions_workflow_renderer::release_gates::release_rust_prepare(
            inputs.catalog,
        )?,
        verify_rust: velnor_actions_workflow_renderer::verifier_builders::rust_verifier_shell_step(
            inputs.catalog,
            velnor_actions_workflow_renderer::verifier_builders::ToolchainPhase::Installed,
        )?,
        tool_env: strings_of_env(&ToolHomes::runner_temp().exec_env(inputs.catalog))
            .map_err(|problem| OrchestratorError::Contract { problem })?,
        executable_env: velnor_actions_workflow_renderer::release_gates::release_executable_env(
            inputs.catalog.version(PinnedTool::Rust),
            inputs.label,
        )?,
        release_plz: velnor_actions_workflow_renderer::release_plz_acquire::acquire_step(
            inputs.catalog.version(PinnedTool::ReleasePlz),
            inputs.label,
        )?,
        semver: velnor_actions_workflow_renderer::release_plz_acquire::acquire_semver_step(
            inputs.label,
        )?,
        label: inputs.label,
        gate: &inputs.gate,
        preparation_gate: &inputs.preparation_gate,
        environment: &inputs.release.environment,
        acquire: inputs.acquire.clone(),
        generator_version: inputs.generator_version,
        rust_version: inputs.catalog.version(PinnedTool::Rust),
    };
    let bootstrap =
        inputs.release.effective_authentication() == ReleaseAuthentication::BootstrapToken;
    let manifest = inputs.release.manifest_path.as_str();
    let mut jobs = BTreeMap::new();
    jobs.insert(
        "release-preparation".to_owned(),
        preparation_job(&parts, inputs.release.release_pr, manifest, inputs.registry)?,
    );
    if inputs.release.release_pr {
        jobs.insert(
            "release-pr-create".to_owned(),
            release_pr_create_job(&parts)?,
        );
    }
    jobs.insert("release-preflight".to_owned(), preflight_job(&parts)?);
    if bootstrap {
        jobs.insert(
            "release-publish-bootstrap".to_owned(),
            bootstrap_job(&parts)?,
        );
    } else {
        jobs.insert("release-publish".to_owned(), publish_job(&parts)?);
    }
    jobs.insert(
        "release-reconcile".to_owned(),
        reconcile_job(&parts, bootstrap)?,
    );
    Ok(jobs)
}

pub(crate) fn phase_argv(
    phase: &Phase,
    config: &str,
    manifest: &str,
    registry: Option<&str>,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    let payload = phase_payload(phase, config, manifest, registry)?;
    let args = payload.into_iter().skip(1).collect();
    let exec = PinnedToolExec::new(
        vec![PinnedTool::Rust, PinnedTool::ReleasePlz],
        OsStr::new("release-plz"),
        args,
    )
    .map_err(map_mise)?;
    strings_of(exec.argv(catalog)).map_err(|problem| OrchestratorError::Contract { problem })
}

fn crate_phase_argv(
    phase: &Phase,
    config: &str,
    manifest: &str,
    registry: Option<&str>,
    auth: velnor_actions_workflow_renderer::release_gates::ReleasePlzAuth,
) -> Result<Vec<String>, OrchestratorError> {
    let payload = phase_payload(phase, config, manifest, registry)?;
    let mut args = vec![velnor_actions_workflow_renderer::release_plz_acquire::BINARY_PATH.into()];
    args.extend(payload.into_iter().skip(1));
    let payload = strings_of(args).map_err(|problem| OrchestratorError::Contract { problem })?;
    Ok(velnor_actions_workflow_renderer::release_gates::isolated_release_plz_argv(payload, auth))
}

fn phase_payload(
    phase: &Phase,
    config: &str,
    manifest: &str,
    registry: Option<&str>,
) -> Result<Vec<OsString>, OrchestratorError> {
    let path = PathBuf::from(config);
    match phase {
        Phase::Publish => PlzRelease::release(path)
            .and_then(|request| request.with_manifest(PathBuf::from(manifest)))
            .and_then(|request| plz_registry(request, registry))
            .map(|request| request.release_argv()),
        Phase::DryRun => PlzRelease::dry_run(path)
            .and_then(|request| request.with_manifest(PathBuf::from(manifest)))
            .and_then(|request| plz_registry(request, registry))
            .map(|request| request.release_argv()),
    }
    .map_err(map_mise)
}

fn plz_registry(
    request: PlzRelease,
    registry: Option<&str>,
) -> Result<PlzRelease, velnor_actions_mise::MiseError> {
    match registry {
        Some(name) => request.with_registry(name),
        None => Ok(request),
    }
}

#[expect(clippy::needless_pass_by_value, reason = "map_err passes owned errors")]
fn map_mise(err: velnor_actions_mise::MiseError) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: err.to_string(),
    }
}

fn preparation_job(
    parts: &JobParts<'_>,
    create_pr: bool,
    manifest: &str,
    registry: Option<&str>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    let mut steps = vec![parts.acquire.clone(), parts.policy.clone()];
    steps.extend([
        parts.setup.clone(),
        parts.prepare_rust.clone(),
        parts.verify_rust.clone(),
        parts.release_plz.clone(),
        parts.semver.clone(),
    ]);
    let mut outputs = BTreeMap::new();
    if create_pr {
        steps.push(
            velnor_actions_workflow_renderer::release_gates::release_pr_candidate_step(
                parts.generator_version,
                parts.rust_version,
                parts.label,
            )?,
        );
        steps.push(parts.verify_rust.clone());
        steps.push(velnor_actions_workflow_renderer::release_gates::release_pr_artifact_step()?);
        outputs = BTreeMap::from([
            (
                "artifact_id".to_owned(),
                "${{ steps.release-pr-artifact.outputs.artifact-id }}".to_owned(),
            ),
            (
                "artifact_digest".to_owned(),
                "${{ steps.release-pr-artifact.outputs.artifact-digest }}".to_owned(),
            ),
            (
                "content_sha256".to_owned(),
                "${{ steps.release-pr-candidate.outputs.content_sha256 }}".to_owned(),
            ),
            (
                "changed".to_owned(),
                "${{ steps.release-pr-candidate.outputs.changed }}".to_owned(),
            ),
        ]);
    } else {
        let argv = crate_phase_argv(
            &Phase::DryRun,
            RELEASE_CONFIG_PATH,
            manifest,
            registry,
            velnor_actions_workflow_renderer::release_gates::ReleasePlzAuth::None,
        )?;
        let mut env = parts.tool_env.clone();
        env.extend(parts.executable_env.clone());
        env.insert("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned());
        steps.push(ambient_shell_step("Validate release", argv, env)?);
    }
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Preparation,
        display_name: "Release preparation".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: vec!["release-preflight".to_owned()],
        outputs,
        condition: Some(parts.preparation_gate.to_owned()),
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Preparation),
        steps,
    })
}

fn release_pr_create_job(parts: &JobParts<'_>) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PrCreate,
        display_name: "Create release PR".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: vec!["release-preparation".to_owned()],
        outputs: BTreeMap::new(),
        condition: Some("needs.release-preparation.result == 'success' && needs.release-preparation.outputs.changed == 'true'".to_owned()),
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::PrCreate),
        steps: vec![parts.acquire.clone(), velnor_actions_workflow_renderer::release_gates::release_pr_writer_step(parts.generator_version)?],
    })
}

const ELIGIBLE: &str = "steps.release-admission.outputs.eligible == 'true'";
const RECEIPT_ARTIFACT: &str =
    "velnor-release-receipt-${{ github.run_id }}-${{ github.run_attempt }}";
const RECEIPT_PATH: &str = "${{ runner.temp }}/velnor-crate-release-receipt.json";
const RUST_ARTIFACT: &str = "velnor-release-rust-${{ github.run_id }}-${{ github.run_attempt }}";
const RUST_ARCHIVE: &str = "${{ runner.temp }}/velnor-rust-toolchain.tar.gz";

fn controller_step(parts: &JobParts<'_>, operation: &str) -> Result<Step, OrchestratorError> {
    let mut env = release_env(parts);
    if operation == "verify-preflight-retention" {
        env.insert(
            "VELNOR_RECEIPT_ARTIFACT_ID".to_owned(),
            "${{ steps.release-receipt-upload.outputs.artifact-id }}".to_owned(),
        );
        env.insert(
            "VELNOR_RUST_ARTIFACT_ID".to_owned(),
            "${{ steps.release-rust-upload.outputs.artifact-id }}".to_owned(),
        );
    }
    if operation == "toolchain-acquire" {
        env.insert(
            "VELNOR_RUST_ARTIFACT_ID".to_owned(),
            "${{ needs.release-preflight.outputs.rust_artifact_id }}".to_owned(),
        );
    }
    if matches!(
        operation,
        "receipt-acquire" | "reconciliation-receipt-acquire"
    ) {
        env.insert(
            "VELNOR_RECEIPT_ARTIFACT_ID".to_owned(),
            "${{ needs.release-preflight.outputs.receipt_artifact_id }}".to_owned(),
        );
    }
    let mut step = ambient_shell_step(
        &format!("Release {operation}"),
        vec![
            format!(
                "$RUNNER_TEMP/velnor/bin/velnor-actions-{}",
                parts.generator_version
            ),
            "crate-release".to_owned(),
            operation.to_owned(),
        ],
        env,
    )?;
    if matches!(operation, "source-admit" | "source-require") {
        step.id = Some(StepId::ReleaseAdmission);
        step.role = Some(StepRole::ReleaseAdmission);
    } else {
        step.condition = Some(ELIGIBLE.to_owned());
        if operation == "verify-admission" {
            step.id = Some(StepId::ReleaseVerification);
            step.role = Some(StepRole::ReleaseVerification);
        }
    }
    Ok(step)
}

fn release_env(parts: &JobParts<'_>) -> BTreeMap<String, String> {
    let mut env = parts.tool_env.clone();
    env.insert("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned());
    env.extend(forge_env());
    env
}

fn conditional(mut step: Step) -> Step {
    step.condition = Some(ELIGIBLE.to_owned());
    step
}

fn receipt_step() -> Result<Step, OrchestratorError> {
    let mut inputs = BTreeMap::from([("name".to_owned(), RECEIPT_ARTIFACT.to_owned())]);
    inputs.insert("path".to_owned(), RECEIPT_PATH.to_owned());
    inputs.insert("if-no-files-found".to_owned(), "error".to_owned());
    inputs.insert("retention-days".to_owned(), "36".to_owned());
    let mut step = action_step("Upload release receipt", UPLOAD_ARTIFACT_USES, inputs)?;
    step.id = Some(StepId::ReleaseReceiptUpload);
    step.role = Some(StepRole::ReleaseReceiptUpload);
    Ok(conditional(step))
}

fn upload_record_step(name: &str, artifact: &str, path: &str) -> Result<Step, OrchestratorError> {
    Ok(conditional(action_step(
        name,
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact.to_owned()),
            ("path".to_owned(), path.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?))
}

#[path = "release_steps_jobs.rs"]
mod jobs;
use jobs::{bootstrap_job, preflight_job, publish_job, reconcile_job};

#[cfg(test)]
#[path = "release_steps_tests.rs"]
pub(crate) mod tests;

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use velnor_actions_contract::config::{ReleaseAuthentication, RustReleaseConfig};
use velnor_actions_contract::{JobTimeout, Step, StepId, StepKind, StepRole};
use velnor_actions_mise::catalog::release_plz::ReleaseRequest as PlzRelease;
use velnor_actions_mise::{PinnedTool, PinnedToolExec, ToolCatalog, ToolHomes};
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseRole};
use velnor_actions_workflow_renderer::release_permissions::JobPermissions;
use velnor_actions_workflow_renderer::release_tree::RELEASE_CONFIG_PATH;
use velnor_actions_workflow_renderer::{
    MiseSetup, action_step, ambient_shell_step, mise_setup_step,
    steps::UPLOAD_ARTIFACT_USES,
};

use crate::OrchestratorError;
use crate::release_checkouts::{forge_env, policy_checkout, source_checkout};
use crate::utf8::{strings_of, strings_of_env};

const BOOTSTRAP_TOKEN_ENV: &str = "CARGO_REGISTRY_TOKEN";
const BOOTSTRAP_SECRET_REF: &str = "${{ secrets.CARGO_REGISTRY_TOKEN }}";

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
}

struct JobParts<'a> {
    policy: Step,
    source: Step,
    source_push: Step,
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
    let parts = JobParts {
        policy: policy_checkout(false)?,
        source: source_checkout("${{ steps.release-admission.outputs.source_sha }}", false)?,
        source_push: source_checkout("${{ steps.release-admission.outputs.source_sha }}", true)?,
        setup: mise_setup_step(inputs.mise)?,
        prepare_rust:
            velnor_actions_workflow_renderer::release_gates::release_rust_prepare(inputs.catalog)?,
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
        semver: velnor_actions_workflow_renderer::release_plz_acquire::acquire_semver_step(inputs.label)?,
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
        jobs.insert("release-pr-create".to_owned(), release_pr_create_job(&parts)?);
    }
    jobs.insert("release-preflight".to_owned(), preflight_job(&parts)?);
    if bootstrap {
        jobs.insert("release-publish-bootstrap".to_owned(), bootstrap_job(&parts)?);
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
    let mut steps = vec![parts.policy.clone()];
    if create_pr {
        steps.push(parts.acquire.clone());
    }
    steps.extend([
        parts.setup.clone(), parts.prepare_rust.clone(), parts.verify_rust.clone(),
        parts.release_plz.clone(), parts.semver.clone(),
    ]);
    let mut outputs = BTreeMap::new();
    if create_pr {
        steps.push(velnor_actions_workflow_renderer::release_gates::release_pr_candidate_step(parts.generator_version, parts.rust_version, parts.label)?);
        steps.push(parts.verify_rust.clone());
        steps.push(velnor_actions_workflow_renderer::release_gates::release_pr_artifact_step()?);
        outputs = BTreeMap::from([
            ("artifact_id".to_owned(), "${{ steps.release-pr-artifact.outputs.artifact-id }}".to_owned()),
            ("artifact_digest".to_owned(), "${{ steps.release-pr-artifact.outputs.artifact-digest }}".to_owned()),
            ("content_sha256".to_owned(), "${{ steps.release-pr-candidate.outputs.content_sha256 }}".to_owned()),
            ("changed".to_owned(), "${{ steps.release-pr-candidate.outputs.changed }}".to_owned()),
        ]);
    } else {
        let argv = crate_phase_argv(
            &Phase::DryRun, RELEASE_CONFIG_PATH, manifest, registry,
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
const RECEIPT_ARTIFACT: &str = "velnor-release-receipt-${{ github.run_id }}-${{ github.run_attempt }}";
const RECEIPT_PATH: &str = "${{ runner.temp }}/velnor-crate-release-receipt.json";
const RUST_ARTIFACT: &str = "velnor-release-rust-${{ github.run_id }}-${{ github.run_attempt }}";
const RUST_ARCHIVE: &str = "${{ runner.temp }}/velnor-rust-toolchain.tar.gz";

fn controller_step(parts: &JobParts<'_>, operation: &str) -> Result<Step, OrchestratorError> {
    let mut env = release_env(parts);
    if operation == "toolchain-acquire" {
        env.insert(
            "VELNOR_RUST_ARTIFACT_ID".to_owned(),
            "${{ needs.release-preflight.outputs.rust_artifact_id }}".to_owned(),
        );
    }
    if matches!(operation, "receipt-acquire" | "reconciliation-receipt-acquire") {
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
    inputs.insert("retention-days".to_owned(), "7".to_owned());
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

fn common_source_steps(
    parts: &JobParts<'_>,
    push: bool,
    require: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = vec![
        parts.policy.clone(),
        parts.acquire.clone(),
        controller_step(
            parts,
            if require {
                "source-require"
            } else {
                "source-admit"
            },
        )?,
        conditional(if push {
            parts.source_push.clone()
        } else {
            parts.source.clone()
        }),
    ];
    if require {
        steps.push(controller_step(parts, "toolchain-acquire")?);
    } else {
        steps.push(parts.setup.clone());
        steps.push(parts.prepare_rust.clone());
    }
    steps.push(parts.verify_rust.clone());
    Ok(steps)
}

fn preflight_job(parts: &JobParts<'_>) -> Result<ReleaseJobSpec, OrchestratorError> {
    let mut steps = common_source_steps(parts, false, false)?;
    steps.push(conditional(parts.semver.clone()));
    steps.push(controller_step(parts, "candidate-receipt")?);
    steps.push(receipt_step()?);
    steps.push(conditional(
        velnor_actions_workflow_renderer::release_gates::release_rust_archive_step(
            parts.rust_version,
            parts.label,
        )?,
    ));
    let mut upload = action_step(
        "Upload release Rust toolchain",
        UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), RUST_ARTIFACT.to_owned()),
            ("path".to_owned(), RUST_ARCHIVE.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?;
    upload.id = Some(StepId::ReleaseRustUpload);
    upload.role = Some(StepRole::ReleaseRustUpload);
    steps.push(conditional(upload));
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Preflight,
        display_name: "Release preflight".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: Vec::new(),
        outputs: BTreeMap::from([
            ("rust_artifact_id".to_owned(), "${{ steps.release-rust-upload.outputs.artifact-id }}".to_owned()),
            ("receipt_artifact_id".to_owned(), "${{ steps.release-receipt-upload.outputs.artifact-id }}".to_owned()),
            (
                "eligible".to_owned(),
                "${{ steps.release-admission.outputs.eligible }}".to_owned(),
            ),
            (
                "source_sha".to_owned(),
                "${{ steps.release-admission.outputs.source_sha }}".to_owned(),
            ),
        ]),
        condition: None,
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Preflight),
        steps,
    })
}

fn publish_steps(
    parts: &JobParts<'_>,
    bootstrap: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let mut steps = common_source_steps(parts, true, true)?;
    steps.push(conditional(parts.semver.clone()));
    steps.push(controller_step(parts, "receipt-acquire")?);
    steps.push(controller_step(parts, "verify-admission")?);
    let coordinate = velnor_actions_workflow_renderer::release_gates::COORDINATE_CONDITION;
    let mut reject = ambient_shell_step(
        "Reject unsupported release action",
        vec!["false".to_owned()],
        BTreeMap::new(),
    )?;
    reject.condition = Some(velnor_actions_workflow_renderer::release_gates::REJECT_MODE_CONDITION.to_owned());
    steps.push(reject);
    let mut acquire_release_plz = parts.release_plz.clone();
    acquire_release_plz.condition = Some(coordinate.to_owned());
    steps.push(acquire_release_plz);
    for operation in ["journal-start", "journal-coordinate"] {
        let mut step = controller_step(parts, operation)?;
        step.condition = Some(coordinate.to_owned());
        if let StepKind::Shell { env, .. } = &mut step.kind {
            env.insert("GH_TOKEN".to_owned(), "${{ secrets.GITHUB_TOKEN }}".to_owned());
            if bootstrap && operation == "journal-coordinate" {
                env.insert(BOOTSTRAP_TOKEN_ENV.to_owned(), BOOTSTRAP_SECRET_REF.to_owned());
            }
        }
        steps.push(step);
    }
    Ok(steps)
}

fn publish_job(
    parts: &JobParts<'_>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PublishOidc,
        display_name: "Release publish".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: vec!["release-preflight".to_owned()],
        outputs: BTreeMap::new(),
        condition: Some(parts.gate.to_owned()),
        environment: Some(parts.environment.to_owned()),
        permissions: JobPermissions::expected(ReleaseRole::PublishOidc),
        steps: publish_steps(parts, false)?,
    })
}

fn bootstrap_job(
    parts: &JobParts<'_>,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    Ok(ReleaseJobSpec {
        role: ReleaseRole::PublishBootstrap,
        display_name: "Release bootstrap publish".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: vec!["release-preflight".to_owned()],
        outputs: BTreeMap::new(),
        condition: Some(parts.gate.to_owned()),
        environment: Some(parts.environment.to_owned()),
        permissions: JobPermissions::expected(ReleaseRole::PublishBootstrap),
        steps: publish_steps(parts, true)?,
    })
}

fn reconcile_job(
    parts: &JobParts<'_>,
    bootstrap: bool,
) -> Result<ReleaseJobSpec, OrchestratorError> {
    let publisher = if bootstrap {
        "release-publish-bootstrap"
    } else {
        "release-publish"
    };
    let mut steps = common_source_steps(parts, false, true)?;
    steps.push(controller_step(parts, "reconciliation-receipt-acquire")?);
    steps.push(controller_step(parts, "reconcile")?);
    steps.push(upload_record_step(
        "Upload reconciliation record",
        "velnor-crate-release-reconciled",
        "${{ runner.temp }}/velnor-crate-release-reconciled.json",
    )?);
    Ok(ReleaseJobSpec {
        role: ReleaseRole::Reconcile,
        display_name: "Release reconcile".to_owned(),
        runs_on: parts.label.to_owned(),
        timeout_minutes: JobTimeout::CRATE_RELEASE,
        needs: vec!["release-preflight".to_owned(), publisher.to_owned()],
        outputs: BTreeMap::new(),
        condition: Some(format!("always() && needs.{publisher}.result != 'skipped'")),
        environment: None,
        permissions: JobPermissions::expected(ReleaseRole::Reconcile),
        steps,
    })
}

#[cfg(test)]
#[path = "release_steps_tests.rs"]
pub(crate) mod tests;

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::{
    RenderError, commands,
    release_checkout_gates::{check_checkout_shape, require_exact_checkout},
    release_jobs::{ReleaseJobSpec, ReleaseRole, ReleaseWorkflowSpec},
    steps,
};

#[path = "release_gates_step_text.rs"]
mod step_text;
use step_text::forbid_substring;

const ADMITTED_SOURCE: &str = "${{ steps.release-admission.outputs.source_sha }}";

pub const COORDINATE_CONDITION: &str = "steps.release-verification.outputs.release_mode == 'publish' || steps.release-verification.outputs.release_mode == 'repair' || steps.release-verification.outputs.release_mode == 'mixed' || steps.release-verification.outputs.release_mode == 'none'";
pub const REJECT_MODE_CONDITION: &str = "steps.release-verification.outputs.release_mode != 'publish' && steps.release-verification.outputs.release_mode != 'repair' && steps.release-verification.outputs.release_mode != 'mixed' && steps.release-verification.outputs.release_mode != 'none'";

pub const GIT_TOKEN_ENV: &str = "GIT_TOKEN";
pub const GIT_TOKEN_REF: &str = "${{ secrets.GITHUB_TOKEN }}";

pub fn release_pr_candidate_step(
    version: &str,
    rust: &str,
    runner: &str,
) -> Result<Step, RenderError> {
    crate::marker::validate_version(version)?;
    let mut step = crate::ambient_shell_step(
        "Prepare release PR candidate",
        vec![
            format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{version}"),
            "crate-release".to_owned(),
            "prepare-pr".to_owned(),
        ],
        release_executable_env(rust, runner)?,
    )?;
    step.id = Some(StepId::ReleasePrCandidate);
    step.role = Some(StepRole::ReleasePrCandidate);
    Ok(step)
}

pub fn release_pr_artifact_step() -> Result<Step, RenderError> {
    let mut step = crate::action_step(
        "Upload release PR candidate",
        crate::steps::UPLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            (
                "name".to_owned(),
                "velnor-release-pr-preparation-${{ github.run_id }}-${{ github.run_attempt }}"
                    .to_owned(),
            ),
            (
                "path".to_owned(),
                "${{ runner.temp }}/velnor-release-pr-preparation.json".to_owned(),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?;
    step.id = Some(StepId::ReleasePrArtifact);
    step.role = Some(StepRole::ReleasePrArtifact);
    step.condition = Some("steps.release-pr-candidate.outputs.changed == 'true'".to_owned());
    Ok(step)
}

pub fn release_pr_writer_step(version: &str) -> Result<Step, RenderError> {
    crate::marker::validate_version(version)?;
    crate::ambient_shell_step(
        "Create release PR",
        vec![
            format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{version}"),
            "crate-release".to_owned(),
            "release-pr-create".to_owned(),
        ],
        std::collections::BTreeMap::from([
            (GIT_TOKEN_ENV.to_owned(), GIT_TOKEN_REF.to_owned()),
            (
                "VELNOR_PR_ARTIFACT_ID".to_owned(),
                "${{ needs.release-preparation.outputs.artifact_id }}".to_owned(),
            ),
            (
                "VELNOR_PR_ARTIFACT_DIGEST".to_owned(),
                "${{ needs.release-preparation.outputs.artifact_digest }}".to_owned(),
            ),
            (
                "VELNOR_PR_CONTENT_SHA256".to_owned(),
                "${{ needs.release-preparation.outputs.content_sha256 }}".to_owned(),
            ),
        ]),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseConfigBinding<'a> {
    pub generator_version: &'a str,
    pub release_plz_version: &'a str,
    pub rust_version: &'a str,
}

pub fn check_release_jobs(
    spec: &ReleaseWorkflowSpec,
    binding: &ReleaseConfigBinding<'_>,
) -> Result<(), RenderError> {
    let catalog = release_catalog(binding.rust_version)?;
    let mut acquire: Option<&Step> = None;
    for (id, job) in &spec.jobs {
        check_job_steps(id, job)?;
        for step in job
            .steps
            .iter()
            .filter(|step| step.role == Some(StepRole::AcquireVelnor))
        {
            if acquire.is_some_and(|prior| prior != step) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_generator_acquire_mismatch:{id}"
                )));
            }
            acquire = Some(step);
        }
        if job.role == ReleaseRole::PrCreate {
            check_pr_writer(id, job, binding.generator_version)?;
            continue;
        }
        check_rust_custody(id, job, &catalog)?;
        if job.role != ReleaseRole::Preparation {
            require_forge_binding(id, &job.steps)?;
        }
        match job.role {
            ReleaseRole::Preparation => {
                check_preparation(id, job, &spec.plan.source_sha, binding)?;
            }
            ReleaseRole::PrCreate => unreachable!(),
            ReleaseRole::Preflight => {
                check_preflight(id, job, ADMITTED_SOURCE, binding.generator_version)?;
            }
            ReleaseRole::PublishOidc => {
                check_publish_oidc(
                    id,
                    job,
                    ADMITTED_SOURCE,
                    binding.generator_version,
                    binding.release_plz_version,
                )?;
            }
            ReleaseRole::PublishBootstrap => {
                check_publish_bootstrap(
                    id,
                    job,
                    ADMITTED_SOURCE,
                    binding.generator_version,
                    binding.release_plz_version,
                )?;
            }
            ReleaseRole::Reconcile => {
                check_reconcile(id, job, ADMITTED_SOURCE, binding.generator_version)?;
            }
        }
    }
    Ok(())
}

fn release_catalog(rust: &str) -> Result<velnor_actions_mise::ToolCatalog, RenderError> {
    use velnor_actions_mise::{PinnedTool, ToolCatalog};
    let base = ToolCatalog::pinned();
    ToolCatalog::new(
        rust,
        base.version(PinnedTool::MrBoxington),
        base.version(PinnedTool::Gh),
        base.version(PinnedTool::Jactionlint),
        base.version(PinnedTool::Shellcheck),
        base.version(PinnedTool::Nextest),
        base.version(PinnedTool::Opentofu),
    )
    .map(|catalog| catalog.with_export_rustup_toolchain(false))
    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))
}

fn check_rust_custody(
    id: &str,
    job: &ReleaseJobSpec,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(), RenderError> {
    use velnor_actions_contract::StepRole;
    let prepare = release_rust_prepare(catalog)?;
    let verify = crate::verifier_builders::rust_verifier_shell_step(
        catalog,
        crate::verifier_builders::ToolchainPhase::Installed,
    )?;
    let prepared: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::PreparePinnedTools))
        .collect();
    let verified: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::VerifyRustManifest))
        .collect();
    let expects_prepare = matches!(job.role, ReleaseRole::Preparation | ReleaseRole::Preflight);
    let preparation_with_pr = job.role == ReleaseRole::Preparation && !job.outputs.is_empty();
    let expected_verifications = if preparation_with_pr {
        vec![&verify, &verify]
    } else {
        vec![&verify]
    };
    if (expects_prepare && prepared != vec![&prepare])
        || (!expects_prepare && !prepared.is_empty())
        || verified != expected_verifications
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_rust_custody:{id}"
        )));
    }
    let prepare_index = if preparation_with_pr { 3 } else { 2 };
    if job.role == ReleaseRole::Preparation
        && (job.steps.get(prepare_index) != Some(&prepare)
            || job.steps.get(prepare_index + 1) != Some(&verify))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_rust_custody_order:{id}"
        )));
    }
    check_release_rust_artifact(id, job, catalog)?;
    check_release_tool_environment(id, job, catalog)
}

fn check_release_rust_artifact(
    id: &str,
    job: &ReleaseJobSpec,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(), RenderError> {
    use velnor_actions_contract::{StepId, StepRole};
    let archives: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ReleaseRustArchive))
        .collect();
    let uploads: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.role == Some(StepRole::ReleaseRustUpload))
        .collect();
    if job.role != ReleaseRole::Preflight {
        if archives.is_empty() && uploads.is_empty() {
            return Ok(());
        }
        return Err(RenderError::InvalidWorkflow(format!(
            "release_rust_artifact:{id}"
        )));
    }
    let condition = Some("steps.release-admission.outputs.eligible == 'true'".to_owned());
    let mut archive = release_rust_archive_step(
        catalog.version(velnor_actions_mise::PinnedTool::Rust),
        &job.runs_on,
    )?;
    archive.condition.clone_from(&condition);
    let mut upload = crate::action_step(
        "Upload release Rust toolchain",
        crate::steps::UPLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            (
                "name".to_owned(),
                "velnor-release-rust-${{ github.run_id }}-${{ github.run_attempt }}".to_owned(),
            ),
            (
                "path".to_owned(),
                "${{ runner.temp }}/velnor-rust-toolchain.tar.gz".to_owned(),
            ),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "7".to_owned()),
        ]),
    )?;
    upload.id = Some(StepId::ReleaseRustUpload);
    upload.role = Some(StepRole::ReleaseRustUpload);
    upload.condition = condition;
    if archives != vec![&archive] || uploads != vec![&upload] {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_rust_artifact:{id}"
        )));
    }
    Ok(())
}

fn add_journal_env(
    role: ReleaseRole,
    step: &str,
    expected: &mut std::collections::BTreeMap<String, String>,
) {
    if matches!(step, "Release journal-start" | "Release journal-coordinate") {
        expected.insert("GH_TOKEN".to_owned(), GIT_TOKEN_REF.to_owned());
    }
    if role == ReleaseRole::PublishBootstrap && step == "Release journal-coordinate" {
        expected.insert(
            "CARGO_REGISTRY_TOKEN".to_owned(),
            "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
        );
    }
}

fn check_release_tool_environment(
    id: &str,
    job: &ReleaseJobSpec,
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<(), RenderError> {
    let mut expected_env = std::collections::BTreeMap::new();
    for (key, value) in velnor_actions_mise::ToolHomes::runner_temp().exec_env(catalog) {
        let key = key
            .into_string()
            .map_err(|_| RenderError::InvalidWorkflow("release_rust_env".to_owned()))?;
        let value = value
            .into_string()
            .map_err(|_| RenderError::InvalidWorkflow("release_rust_env".to_owned()))?;
        expected_env.insert(key, value);
    }
    if job.role != ReleaseRole::Preparation {
        expected_env.insert(GIT_TOKEN_ENV.to_owned(), GIT_TOKEN_REF.to_owned());
    }
    expected_env.insert("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned());
    for step in &job.steps {
        if (step.name.starts_with("Release ")
            || step.name == "Publish release"
            || step.name == "Prepare release PR"
            || step.name == "Validate release")
            && let StepKind::Shell { run, env } = &step.kind
        {
            let mut expected = expected_env.clone();
            if step.name == "Release toolchain-acquire" {
                expected.insert(
                    "VELNOR_RUST_ARTIFACT_ID".to_owned(),
                    "${{ needs.release-preflight.outputs.rust_artifact_id }}".to_owned(),
                );
            }
            if matches!(
                step.name.as_str(),
                "Release receipt-acquire" | "Release reconciliation-receipt-acquire"
            ) {
                expected.insert(
                    "VELNOR_RECEIPT_ARTIFACT_ID".to_owned(),
                    "${{ needs.release-preflight.outputs.receipt_artifact_id }}".to_owned(),
                );
            }
            if matches!(
                step.name.as_str(),
                "Publish release" | "Prepare release PR" | "Validate release"
            ) {
                expected.extend(release_executable_env(
                    catalog.version(velnor_actions_mise::PinnedTool::Rust),
                    &job.runs_on,
                )?);
            }
            add_journal_env(job.role, &step.name, &mut expected);
            if *env != expected {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_rust_env:{id}:{}",
                    step.name
                )));
            }
            if matches!(
                step.name.as_str(),
                "Publish release" | "Prepare release PR" | "Validate release"
            ) {
                let auth = match job.role {
                    ReleaseRole::PublishBootstrap => ReleasePlzAuth::Bootstrap,
                    ReleaseRole::PublishOidc => ReleasePlzAuth::Trusted,
                    _ => ReleasePlzAuth::None,
                };
                let prefix = isolated_release_plz_argv(Vec::new(), auth);
                if !run.starts_with(&prefix)
                    || run.get(prefix.len()).map(String::as_str)
                        != Some(crate::release_plz_acquire::BINARY_PATH)
                {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "release_tool_isolation:{id}:{}",
                        step.name
                    )));
                }
                render_release_plz_argv(run)?;
            }
        }
    }
    Ok(())
}

pub fn render_release_plz_argv(argv: &[String]) -> Result<String, RenderError> {
    commands::join_validated_argv_for_run(argv)
}

pub fn release_executable_env(
    version: &str,
    runner: &str,
) -> Result<std::collections::BTreeMap<String, String>, RenderError> {
    release_catalog(version)?;
    let target = velnor_actions_contract::ReleaseTarget::for_runner_label(runner)
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_rust_runner:{runner}")))?;
    let bin = format!(
        "${{{{ runner.temp }}}}/velnor/rustup/toolchains/{version}-{}/bin",
        target.triple()
    );
    Ok(std::collections::BTreeMap::from([
        (
            "PATH".to_owned(),
            format!("${{{{ runner.temp }}}}/velnor-semver:{bin}:/usr/bin:/bin"),
        ),
        ("CARGO".to_owned(), format!("{bin}/cargo")),
        ("RUSTC".to_owned(), format!("{bin}/rustc")),
        ("RUSTDOC".to_owned(), format!("{bin}/rustdoc")),
    ]))
}

pub fn release_rust_archive_step(version: &str, runner: &str) -> Result<Step, RenderError> {
    release_catalog(version)?;
    let target = velnor_actions_contract::ReleaseTarget::for_runner_label(runner)
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_rust_runner:{runner}")))?;
    let script = format!(
        "set -eu; umask 077; test ! -e \"$RUNNER_TEMP/velnor-rust-toolchain.tar.gz\"; PATH=/usr/bin:/bin; export PATH; /usr/bin/tar -czf \"$RUNNER_TEMP/velnor-rust-toolchain.tar.gz\" -C \"$RUNNER_TEMP/velnor/rustup\" 'toolchains/{version}-{}'",
        target.triple()
    );
    let mut step = crate::shell_step(
        "Archive release Rust toolchain",
        vec!["sh".to_owned(), "-c".to_owned(), script],
        std::collections::BTreeMap::new(),
    )?;
    step.role = Some(velnor_actions_contract::StepRole::ReleaseRustArchive);
    Ok(step)
}

#[derive(Debug, Clone, Copy)]
pub enum ReleasePlzAuth {
    None,
    Trusted,
    Bootstrap,
}

#[must_use]
pub fn isolated_release_plz_argv(payload: Vec<String>, auth: ReleasePlzAuth) -> Vec<String> {
    let names = [
        "PATH",
        "CARGO",
        "RUSTC",
        "RUSTDOC",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "RUSTUP_AUTO_INSTALL",
        "HOME",
        "RUNNER_TEMP",
        "GITHUB_WORKSPACE",
        "GITHUB_REPOSITORY",
        "GITHUB_SHA",
        "GITHUB_REF",
        "GITHUB_EVENT_NAME",
        "GITHUB_EVENT_PATH",
        "GITHUB_SERVER_URL",
        "GITHUB_API_URL",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
    ];
    let mut argv = vec!["/usr/bin/env".to_owned(), "-i".to_owned()];
    argv.extend(names.iter().map(|name| format!("{name}=${{{name}-}}")));
    match auth {
        ReleasePlzAuth::None => {}
        ReleasePlzAuth::Trusted => argv.extend([
            "GIT_TOKEN=${GIT_TOKEN-}".to_owned(),
            "ACTIONS_ID_TOKEN_REQUEST_URL=${ACTIONS_ID_TOKEN_REQUEST_URL-}".to_owned(),
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN=${ACTIONS_ID_TOKEN_REQUEST_TOKEN-}".to_owned(),
        ]),
        ReleasePlzAuth::Bootstrap => {
            argv.push("GIT_TOKEN=${GIT_TOKEN-}".to_owned());
            argv.push("CARGO_REGISTRY_TOKEN=${CARGO_REGISTRY_TOKEN-}".to_owned());
        }
    }
    argv.extend(payload);
    argv
}

pub fn release_rust_prepare(
    catalog: &velnor_actions_mise::ToolCatalog,
) -> Result<Step, RenderError> {
    let prepared = crate::native_job_builders::prepare_pinned_tools_step(
        catalog,
        vec![velnor_actions_mise::PinnedTool::Rust],
        true,
    )
    .map_err(|error| RenderError::InvalidWorkflow(format!("rust_prepare:{error:?}")))?;
    let StepKind::Shell { run, env } = prepared.kind else {
        return Err(RenderError::InvalidWorkflow(
            "release_rust_prepare_kind".to_owned(),
        ));
    };
    let mut isolated = crate::shell_step(&prepared.name, run, env)?;
    isolated.role = Some(velnor_actions_contract::StepRole::PreparePinnedTools);
    Ok(isolated)
}

fn check_job_steps(id: &str, job: &ReleaseJobSpec) -> Result<(), RenderError> {
    for step in &job.steps {
        match &step.kind {
            StepKind::Action { uses, with, env } => {
                steps::validate_uses(uses)?;
                for entry in with.keys().chain(with.values()) {
                    steps::scan_for_private_subcommands(entry)?;
                }
                commands::validate_env(env)?;
            }
            StepKind::Shell { run, env } => {
                commands::validate_command_argv(run)?;
                commands::validate_env(env)?;
            }
            StepKind::Internal { .. } => {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_internal_op:{id}"
                )));
            }
            StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. } => {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_declared_task:{id}"
                )));
            }
        }
    }
    Ok(())
}

fn is_forge_binding(key: &str, value: &str) -> bool {
    key == GIT_TOKEN_ENV && value == GIT_TOKEN_REF
}

fn forbid_unexpected_secrets(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    let rejected = || RenderError::InvalidWorkflow(format!("secret_outside_bootstrap:{id}"));
    for step in steps {
        match &step.kind {
            StepKind::Action { with, .. } => {
                if with.values().any(|value| value.contains("secrets.")) {
                    return Err(rejected());
                }
            }
            StepKind::Shell { run, env } => {
                if run.iter().any(|arg| arg.contains("secrets.")) {
                    return Err(rejected());
                }
                if env.iter().any(|(key, value)| {
                    value.contains("secrets.")
                        && !is_forge_binding(key, value)
                        && !(matches!(
                            step.name.as_str(),
                            "Release journal-start" | "Release journal-coordinate"
                        ) && key == "GH_TOKEN"
                            && value == GIT_TOKEN_REF)
                }) {
                    return Err(rejected());
                }
            }
            StepKind::Internal { .. } => {}
            StepKind::TaskExecution { argv, env, .. } => {
                if argv.iter().any(|arg| arg.contains("secrets."))
                    || env.iter().any(|(key, value)| {
                        value.contains("secrets.") && !is_forge_binding(key, value)
                    })
                {
                    return Err(rejected());
                }
            }
            StepKind::NativeCheckExecution {
                check_id,
                task_id,
                sdk_version,
                ..
            } => {
                if [check_id, task_id, sdk_version]
                    .iter()
                    .any(|value| value.contains("secrets."))
                {
                    return Err(rejected());
                }
            }
        }
    }
    Ok(())
}

fn require_forge_binding(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps {
        if let StepKind::Shell { run, env } = &step.kind
            && run
                .iter()
                .any(|arg| arg == crate::release_plz_acquire::BINARY_PATH)
            && !env.iter().any(|(key, value)| is_forge_binding(key, value))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "missing_forge_binding:{id}"
            )));
        }
    }
    Ok(())
}

fn forbid_registry_token(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps {
        let rejected = match &step.kind {
            StepKind::Shell { run, env } => {
                (step.role != Some(velnor_actions_contract::StepRole::VerifyRustManifest)
                    && run.iter().any(|part| part.contains("CARGO_REGISTRY_TOKEN")))
                    || env.get("CARGO_REGISTRY_TOKEN").is_some_and(|value| {
                        !value.is_empty()
                            || !matches!(
                            step.role,
                            Some(
                                velnor_actions_contract::StepRole::AcquireVelnor
                                    | velnor_actions_contract::StepRole::AcquireReleasePlz
                                    | velnor_actions_contract::StepRole::AcquireCargoSemverChecks
                                    | velnor_actions_contract::StepRole::VerifyRustManifest
                                    | velnor_actions_contract::StepRole::PreparePinnedTools
                                    | velnor_actions_contract::StepRole::ReleaseRustArchive
                            )
                        )
                    })
            }
            StepKind::Action { uses, with, env } => {
                uses.contains("CARGO_REGISTRY_TOKEN")
                    || with.iter().any(|(key, value)| {
                        key.contains("CARGO_REGISTRY_TOKEN")
                            || value.contains("CARGO_REGISTRY_TOKEN")
                    })
                    || env.get("CARGO_REGISTRY_TOKEN").is_some_and(|value| {
                        !value.is_empty()
                            || !matches!(
                            step.role,
                            Some(
                                velnor_actions_contract::StepRole::AcquireVelnor
                                    | velnor_actions_contract::StepRole::AcquireReleasePlz
                                    | velnor_actions_contract::StepRole::AcquireCargoSemverChecks
                            )
                        )
                    })
            }
            _ => true,
        };
        if rejected {
            return Err(RenderError::InvalidWorkflow(format!(
                "registry_token_outside_bootstrap:{id}"
            )));
        }
    }
    Ok(())
}

fn forbid_inputs_interpolation(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    forbid_substring(id, steps, "github.event", "dispatch_input_in_steps")?;
    forbid_substring(id, steps, "inputs.", "dispatch_input_in_steps")
}

fn check_preparation(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    binding: &ReleaseConfigBinding<'_>,
) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)?;
    let has_pr = !job.outputs.is_empty();
    let offset = usize::from(has_pr);
    if job.steps.len() != if has_pr { 10 } else { 7 }
        || job.steps[4 + offset]
            != crate::release_plz_acquire::acquire_step(binding.release_plz_version, &job.runs_on)?
        || job.steps[5 + offset] != crate::release_plz_acquire::acquire_semver_step(&job.runs_on)?
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_plz_acquire:{id}"
        )));
    }
    if has_pr {
        check_acquire_step(id, &job.steps[1], binding.generator_version, &job.runs_on)?;
        if job.steps[7]
            != release_pr_candidate_step(
                binding.generator_version,
                binding.rust_version,
                &job.runs_on,
            )?
            || job.steps[9] != release_pr_artifact_step()?
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_pr_preparation:{id}"
            )));
        }
    } else if job.steps[6].name != "Validate release" {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_pr_disabled:{id}"
        )));
    }
    Ok(())
}

fn check_acquire_step(
    id: &str,
    step: &Step,
    version: &str,
    runner: &str,
) -> Result<(), RenderError> {
    let StepKind::Shell { env, .. } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_generator_acquire:{id}"
        )));
    };
    let source = env
        .get("VELNOR_GENERATOR_SOURCE_SHA")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_generator_acquire:{id}")))?;
    let manifest = env
        .get("VELNOR_GENERATOR_MANIFEST_SHA256")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_generator_acquire:{id}")))?;
    if *step != crate::release_generator_bootstrap::acquire_step(version, runner, source, manifest)?
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_generator_acquire:{id}"
        )));
    }
    Ok(())
}

fn check_pr_writer(id: &str, job: &ReleaseJobSpec, version: &str) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    if job.steps.len() != 2 || job.steps[1] != release_pr_writer_step(version)? {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_pr_writer:{id}"
        )));
    }
    check_acquire_step(id, &job.steps[0], version, &job.runs_on)
}

fn check_preflight(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    version: &str,
) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)?;
    require_exact_checkout(id, &job.steps, sha)?;
    check_runtime_steps(id, job, version, None)
}

const VERIFY_BYPASS_TOKENS: &[&str] = &["allow-dirty", "allow_dirty", "no-verify", "no_verify"];

fn forbid_verify_bypass(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps {
        if let StepKind::Shell { run, .. } = &step.kind {
            for arg in run {
                for token in VERIFY_BYPASS_TOKENS {
                    if arg.contains(token) {
                        return Err(RenderError::InvalidWorkflow(format!("verify_bypass:{id}")));
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_publish_common(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    version: &str,
    plz_version: &str,
) -> Result<(), RenderError> {
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)?;
    require_exact_checkout(id, &job.steps, sha)?;
    forbid_verify_bypass(id, &job.steps)?;
    check_runtime_steps(id, job, version, Some(plz_version))
}

fn check_publish_oidc(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    version: &str,
    plz_version: &str,
) -> Result<(), RenderError> {
    check_publish_common(id, job, sha, version, plz_version)?;
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)
}

fn is_secret_ref(value: &str) -> bool {
    value
        .strip_prefix("${{ secrets.")
        .and_then(|rest| rest.strip_suffix(" }}"))
        .is_some_and(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
}

fn check_publish_bootstrap(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    version: &str,
    plz_version: &str,
) -> Result<(), RenderError> {
    check_publish_common(id, job, sha, version, plz_version)?;
    let mut bindings = 0_u32;
    for step in &job.steps {
        match &step.kind {
            StepKind::Shell { run, env } => {
                for arg in run {
                    if arg.contains("secrets.") {
                        return Err(RenderError::InvalidWorkflow(format!("secret_in_argv:{id}")));
                    }
                }
                for (key, value) in env {
                    if !value.contains("secrets.")
                        || is_forge_binding(key, value)
                        || (matches!(
                            step.name.as_str(),
                            "Release journal-start" | "Release journal-coordinate"
                        ) && key == "GH_TOKEN"
                            && value == GIT_TOKEN_REF)
                    {
                        continue;
                    }
                    if key != "CARGO_REGISTRY_TOKEN" || !is_secret_ref(value) {
                        return Err(RenderError::InvalidWorkflow(format!(
                            "bootstrap_token_binding:{id}"
                        )));
                    }
                    bindings += 1;
                }
            }
            StepKind::Action { with, .. } => {
                for value in with.values() {
                    if value.contains("secrets.") {
                        return Err(RenderError::InvalidWorkflow(format!(
                            "secret_in_action_input:{id}"
                        )));
                    }
                }
            }
            StepKind::Internal { .. } => {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_internal_op:{id}"
                )));
            }
            StepKind::TaskExecution { .. } | StepKind::NativeCheckExecution { .. } => {
                return Err(RenderError::InvalidWorkflow(format!(
                    "release_declared_task:{id}"
                )));
            }
        }
    }
    if bindings == 1 {
        Ok(())
    } else {
        Err(RenderError::InvalidWorkflow(format!(
            "bootstrap_token_binding:{id}"
        )))
    }
}

fn check_reconcile(
    id: &str,
    job: &ReleaseJobSpec,
    sha: &str,
    version: &str,
) -> Result<(), RenderError> {
    forbid_unexpected_secrets(id, &job.steps)?;
    forbid_registry_token(id, &job.steps)?;
    forbid_inputs_interpolation(id, &job.steps)?;
    check_checkout_shape(id, &job.steps, job.role, sha)?;
    require_exact_checkout(id, &job.steps, sha)?;
    check_runtime_steps(id, job, version, None)
}

fn runtime_step_names(role: ReleaseRole) -> &'static [&'static str] {
    match role {
        ReleaseRole::Preflight => &[
            "Checkout",
            "Acquire Velnor",
            "Release source-admit",
            "Checkout exact source",
            "Setup Mise",
            "Prepare pinned tools",
            "Verify Rust channel manifest",
            "Acquire cargo-semver-checks",
            "Release candidate-receipt",
            "Upload release receipt",
            "Archive release Rust toolchain",
            "Upload release Rust toolchain",
        ],
        ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap => &[
            "Checkout",
            "Acquire Velnor",
            "Release source-require",
            "Checkout exact source",
            "Release toolchain-acquire",
            "Verify Rust channel manifest",
            "Acquire cargo-semver-checks",
            "Release receipt-acquire",
            "Release verify-admission",
            "Reject unsupported release action",
            "Acquire release-plz",
            "Release journal-start",
            "Release journal-coordinate",
        ],
        ReleaseRole::Reconcile => &[
            "Checkout",
            "Acquire Velnor",
            "Release source-require",
            "Checkout exact source",
            "Release toolchain-acquire",
            "Verify Rust channel manifest",
            "Release reconciliation-receipt-acquire",
            "Release reconcile",
            "Upload reconciliation record",
        ],
        ReleaseRole::Preparation | ReleaseRole::PrCreate => &[],
    }
}

fn check_runtime_steps(
    id: &str,
    job: &ReleaseJobSpec,
    version: &str,
    plz_version: Option<&str>,
) -> Result<(), RenderError> {
    let expected = runtime_step_names(job.role);
    if job.steps.len() != expected.len()
        || job
            .steps
            .iter()
            .zip(expected)
            .any(|(step, name)| step.name != *name)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_step_sequence:{id}"
        )));
    }
    let velnor_actions_contract::StepKind::Shell { env, .. } = &job.steps[1].kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_generator_acquire:{id}"
        )));
    };
    let source_sha = env
        .get("VELNOR_GENERATOR_SOURCE_SHA")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_generator_acquire:{id}")))?;
    let manifest_sha = env
        .get("VELNOR_GENERATOR_MANIFEST_SHA256")
        .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_generator_acquire:{id}")))?;
    if job.steps[1]
        != crate::release_generator_bootstrap::acquire_step(
            version,
            &job.runs_on,
            source_sha,
            manifest_sha,
        )?
        || job.steps[1].role != Some(velnor_actions_contract::StepRole::AcquireVelnor)
        || job.steps[2].id != Some(velnor_actions_contract::StepId::ReleaseAdmission)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_source_admission:{id}"
        )));
    }
    for (index, step) in job.steps.iter().enumerate() {
        check_runtime_condition(id, index, step, job.role)?;
        check_runtime_payload(id, step, version, plz_version, &job.runs_on, job.role)?;
    }
    Ok(())
}

fn check_runtime_condition(
    id: &str,
    index: usize,
    step: &Step,
    _role: ReleaseRole,
) -> Result<(), RenderError> {
    let expected = match step.name.as_str() {
        "Setup Mise" | "Prepare pinned tools" | "Verify Rust channel manifest" => None,
        "Reject unsupported release action" => Some(REJECT_MODE_CONDITION),
        "Release journal-start" | "Release journal-coordinate" | "Acquire release-plz" => {
            Some(COORDINATE_CONDITION)
        }
        _ => (index >= 3).then_some("steps.release-admission.outputs.eligible == 'true'"),
    };
    if step.condition.as_deref() != expected {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_step_condition:{id}:{}",
            step.name
        )));
    }
    Ok(())
}

fn check_runtime_payload(
    id: &str,
    step: &Step,
    version: &str,
    plz_version: Option<&str>,
    runner: &str,
    _role: ReleaseRole,
) -> Result<(), RenderError> {
    if step.name == "Acquire cargo-semver-checks" {
        let mut expected = crate::release_plz_acquire::acquire_semver_step(runner)?;
        expected.condition = Some("steps.release-admission.outputs.eligible == 'true'".to_owned());
        if *step != expected {
            return Err(RenderError::InvalidWorkflow(format!(
                "cargo_semver_acquire:{id}"
            )));
        }
    }
    if step.name == "Acquire release-plz" {
        let version = plz_version
            .ok_or_else(|| RenderError::InvalidWorkflow(format!("release_plz_version:{id}")))?;
        let mut expected = crate::release_plz_acquire::acquire_step(version, runner)?;
        expected.condition = Some(COORDINATE_CONDITION.to_owned());
        if *step != expected {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_plz_acquire:{id}"
            )));
        }
    }
    if step.name == "Release verify-admission"
        && step.id != Some(velnor_actions_contract::StepId::ReleaseVerification)
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_verification_output:{id}"
        )));
    }
    if step.name == "Reject unsupported release action"
        && !matches!(&step.kind, StepKind::Shell { run, env } if run == &["false"] && env.is_empty())
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_unknown_action:{id}"
        )));
    }
    if let Some(operation) = match step.name.as_str() {
        "Release source-admit" => Some("source-admit"),
        "Release source-require" => Some("source-require"),
        "Release toolchain-acquire" => Some("toolchain-acquire"),
        "Release receipt-acquire" => Some("receipt-acquire"),
        "Release reconciliation-receipt-acquire" => Some("reconciliation-receipt-acquire"),
        "Release candidate-receipt" => Some("candidate-receipt"),
        "Release verify-admission" => Some("verify-admission"),
        "Release journal-start" => Some("journal-start"),
        "Release journal-coordinate" => Some("journal-coordinate"),
        "Release reconcile" => Some("reconcile"),
        _ => None,
    } {
        let StepKind::Shell { run, env } = &step.kind else {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_command:{id}"
            )));
        };
        if run.len() != 3
            || run[0] != format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{version}")
            || run[1] != "crate-release"
            || run[2] != operation
            || !env.iter().any(|(key, value)| is_forge_binding(key, value))
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_command:{id}"
            )));
        }
    }
    check_receipt_step(id, step)
}

fn check_receipt_step(id: &str, step: &Step) -> Result<(), RenderError> {
    if !matches!(
        step.name.as_str(),
        "Upload release receipt" | "Upload publication record" | "Upload reconciliation record"
    ) {
        return Ok(());
    }
    let StepKind::Action { uses, with, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_receipt:{id}"
        )));
    };
    let expected_action = crate::steps::UPLOAD_ARTIFACT_USES;
    let (expected_artifact, expected_path) = match step.name.as_str() {
        "Upload publication record" => (
            "velnor-crate-publication-started",
            "${{ runner.temp }}/velnor-crate-release-publication-started.json",
        ),
        "Upload reconciliation record" => (
            "velnor-crate-release-reconciled",
            "${{ runner.temp }}/velnor-crate-release-reconciled.json",
        ),
        "Upload release receipt" => (
            "velnor-release-receipt-${{ github.run_id }}-${{ github.run_attempt }}",
            "${{ runner.temp }}/velnor-crate-release-receipt.json",
        ),
        _ => {
            return Err(RenderError::InvalidWorkflow(format!(
                "release_receipt:{id}"
            )));
        }
    };
    if uses != expected_action
        || !env.is_empty()
        || with.get("name").map(String::as_str) != Some(expected_artifact)
        || with.get("path").map(String::as_str) != Some(expected_path)
        || with.len() != 4
        || with.get("if-no-files-found").map(String::as_str) != Some("error")
        || with.get("retention-days").map(String::as_str) != Some("7")
        || (step.name == "Upload release receipt"
            && (step.id != Some(StepId::ReleaseReceiptUpload)
                || step.role != Some(StepRole::ReleaseReceiptUpload)))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "release_receipt:{id}"
        )));
    }
    Ok(())
}

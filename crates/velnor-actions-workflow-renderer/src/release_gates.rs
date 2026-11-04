// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::{
    RenderError, commands,
    release_checkout_gates::check_checkout_shape,
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

#[path = "release_gates_custody.rs"]
mod custody;
use custody::{check_rust_custody, release_catalog};

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

#[path = "release_gates_secrets.rs"]
mod secrets;
use secrets::{
    check_job_steps, forbid_inputs_interpolation, forbid_registry_token, forbid_unexpected_secrets,
    is_forge_binding, require_forge_binding,
};

#[path = "release_gates_roles.rs"]
mod roles;
use roles::{
    check_pr_writer, check_preflight, check_preparation, check_publish_bootstrap,
    check_publish_oidc,
};

#[path = "release_gates_runtime.rs"]
mod runtime;
use runtime::{check_reconcile, check_runtime_steps};

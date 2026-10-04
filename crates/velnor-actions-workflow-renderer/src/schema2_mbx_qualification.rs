//! Hosted-only MBX cache qualification jobs.
//!
//! The protected-main writer, read-only readers, and parallel cache-key probes
//! use the production restore/import/export/save graph and private job stores.

use std::collections::BTreeMap;

use super::MbxQualificationPins;
use super::features::{base, finish, gated};
use super::mbx_qualification_helpers::{
    COMPILE_STEP_NAME, CORRUPT_JOB_ID, QUALIFICATION_CACHE_SCOPE, QualificationRole, READER_JOB_ID,
    WRITER_JOB_ID, permission_yaml, qualification_env,
};
use crate::cache_steps::MBX_ACTION_NAME;
use crate::render::RenderContext;
use crate::steps::{self, MBX_SETUP_NAME};
use crate::yaml::Yaml;
use crate::{RenderError, mbx_bundle};
use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, PullRequestCachePolicy, Step,
};

const QUALIFICATION_GATE: &str = "inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true";
pub(super) const MBX_CACHE_ACTION_STEP: &str = MBX_SETUP_NAME;
const CACHE_COMPRESSION_PROBE: &str = r#"set -eu; command -v zstd >/dev/null; zstd --version > "$RUNNER_TEMP/mbx-cache-zstd-version"; tar --version > "$RUNNER_TEMP/mbx-cache-tar-version"; grep -Fq 'GNU tar' "$RUNNER_TEMP/mbx-cache-tar-version""#;
const SMOKE_CRATE: &str = r#"set -eu; root="$GITHUB_WORKSPACE/.velnor-mbx-cache-qualification"; mkdir -p "$root/src"; printf '[package]\nname = "mbx-cache-qualification"\nversion = "0.1.0"\nedition = "2024"\n\n[workspace]\nmembers = ["."]\nresolver = "3"\n\n[lib]\npath = "src/lib.rs"\n' > "$root/Cargo.toml"; printf 'pub fn cache_probe() -> u64 { 42 }\n' > "$root/src/lib.rs"; mbx build --manifest-path "$root/Cargo.toml""#;
const IMPORT_PROBE: &str = "mbx cache stats --json | jq -e '.objects > 0' >/dev/null";
const REUSE_PROBE: &str = "mbx stats --json | jq -e '.savings.cached_compilations > 0' >/dev/null";
const COLD_REUSE_PROBE: &str =
    "mbx stats --json | jq -e '.savings.cached_compilations == 0' >/dev/null";

/// Emit roundtrip, corruption, and parallel jobs for the pinned MBX runtime.
///
/// # Errors
/// Invalid action, Mise, MBX, Rust, or hosted-runner inputs fail closed.
pub(super) fn jobs(
    request: &MbxQualificationPins,
    hosted: &Yaml,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    request.mise_setup.validate()?;
    crate::steps::validate_uses(&request.mbx_action_uses)?;
    let action_prefix = format!("{MBX_ACTION_NAME}@");
    if !request.mbx_action_uses.starts_with(&action_prefix) {
        return Err(RenderError::BadActionRef(format!(
            "not_mbx_action:{}",
            request.mbx_action_uses
        )));
    }
    validate_exact_version(&request.mbx_version, "mbx")?;
    validate_exact_version(&request.rust_version, "rust")?;

    let mut typed_jobs = BTreeMap::from([
        (
            WRITER_JOB_ID.to_owned(),
            typed_job(request, hosted, QualificationRole::Writer)?,
        ),
        (
            READER_JOB_ID.to_owned(),
            typed_job(request, hosted, QualificationRole::Reader)?,
        ),
        (
            CORRUPT_JOB_ID.to_owned(),
            typed_job(request, hosted, QualificationRole::CorruptReader)?,
        ),
    ]);
    mbx_bundle::append_single_bundle_saves(&mut typed_jobs, PullRequestCachePolicy::ReadOnly)?;
    super::mbx_resource_probe::attach(&mut typed_jobs, request)?;
    let Some(writer) = typed_jobs.remove(WRITER_JOB_ID) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "missing_mbx_job:{WRITER_JOB_ID}"
        )));
    };
    let Some(reader) = typed_jobs.remove(READER_JOB_ID) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "missing_mbx_job:{READER_JOB_ID}"
        )));
    };
    let Some(corrupt_reader) = typed_jobs.remove(CORRUPT_JOB_ID) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "missing_mbx_job:{CORRUPT_JOB_ID}"
        )));
    };
    let mut jobs = vec![
        render_job(writer, QualificationRole::Writer, request)?,
        render_job(reader, QualificationRole::Reader, request)?,
        render_job(corrupt_reader, QualificationRole::CorruptReader, request)?,
    ];
    jobs.extend(super::mbx_parallel_probe::jobs(request, hosted)?);
    jobs.push(super::mbx_roundtrip_terminal::job(request, hosted)?);
    jobs.extend(super::mbx_cancel_probe::jobs(request, hosted)?);
    Ok(jobs)
}

fn typed_job(
    request: &MbxQualificationPins,
    hosted: &Yaml,
    role: QualificationRole,
) -> Result<Job, RenderError> {
    let (title, needs) = match role {
        QualificationRole::Writer => ("MBX objects cache / protected-main writer", Vec::new()),
        QualificationRole::Reader => (
            "MBX objects cache / read-only reuse",
            vec![WRITER_JOB_ID.to_owned()],
        ),
        QualificationRole::CorruptReader => (
            "MBX objects cache / corrupt import cold fallback",
            vec![WRITER_JOB_ID.to_owned(), READER_JOB_ID.to_owned()],
        ),
    };
    let permissions = Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: if role.is_writer() {
            PermissionLevel::Write
        } else {
            PermissionLevel::Read
        },
    };
    let timeout =
        JobTimeout::new(45).map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let runs_on = match hosted {
        Yaml::Str(label) => label.clone(),
        _ => {
            return Err(RenderError::InvalidWorkflow(
                "mbx_qualification_requires_hosted_runner".to_owned(),
            ));
        }
    };
    Ok(Job {
        display_name: title.to_owned(),
        runs_on,
        timeout_minutes: timeout,
        needs,
        condition: Some(QUALIFICATION_GATE.to_owned()),
        permissions: Some(permissions),
        environment: None,
        steps: qualification_steps(request, role)?,
    })
}

fn render_job(
    mut typed_job: Job,
    role: QualificationRole,
    request: &MbxQualificationPins,
) -> Result<(String, Yaml), RenderError> {
    let id = match role {
        QualificationRole::Writer => WRITER_JOB_ID,
        QualificationRole::Reader => READER_JOB_ID,
        QualificationRole::CorruptReader => CORRUPT_JOB_ID,
    };
    let title = typed_job.display_name.clone();
    if role.is_writer() {
        gate_writer_steps(&mut typed_job.steps);
    }
    let rendered_steps = super::mbx_resource_probe_render::render_job_steps(
        id,
        &typed_job.steps,
        request,
        &step_context(),
    )?;

    let mut fields = base(&title, Yaml::str(typed_job.runs_on.clone()), 45);
    if !typed_job.needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(typed_job.needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    fields.push(("permissions".to_owned(), permission_yaml(role.is_writer())));
    fields.push(("env".to_owned(), qualification_env(request)));
    Ok(gated(
        finish(id, fields, rendered_steps),
        QUALIFICATION_GATE,
    ))
}

fn qualification_steps(
    request: &MbxQualificationPins,
    role: QualificationRole,
) -> Result<Vec<Step>, RenderError> {
    let writer = role.is_writer();
    let mut steps = vec![
        checkout_step()?,
        mise_setup_step(request)?,
        mise_install_step(request)?,
        shell_step(
            request,
            "Verify cache compression support",
            CACHE_COMPRESSION_PROBE,
        )?,
        mbx_action_step(request, writer)?,
        verify_action_step(request)?,
    ];
    if role.is_regular_reader() {
        steps.push(shell_step(
            request,
            "Require imported MBX objects",
            IMPORT_PROBE,
        )?);
    }
    steps.push(shell_step(request, COMPILE_STEP_NAME, SMOKE_CRATE)?);
    if role.is_regular_reader() {
        steps.push(shell_step(
            request,
            "Require reused compilation",
            REUSE_PROBE,
        )?);
    } else if role == QualificationRole::CorruptReader {
        steps.push(shell_step(
            request,
            "Require cold compilation after corrupt import",
            COLD_REUSE_PROBE,
        )?);
    }
    Ok(steps)
}

fn checkout_step() -> Result<Step, RenderError> {
    steps::checkout_step(super::features::CHECKOUT_USES)
}

fn mise_setup_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    steps::action_step(
        "Set up Mise",
        &request.mise_setup.uses,
        BTreeMap::from([
            ("version".to_owned(), request.mise_setup.version.clone()),
            ("sha256".to_owned(), request.mise_setup.sha256.clone()),
            ("install".to_owned(), "false".to_owned()),
            ("env".to_owned(), "false".to_owned()),
            ("cache".to_owned(), "false".to_owned()),
            ("cache_save".to_owned(), "false".to_owned()),
        ]),
    )
}

fn mise_install_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    let rust = &request.rust_version;
    steps::shell_step(
        "Install pinned Rust toolchain",
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            format!(
                "mise install rust@{rust} && mise exec rust@{rust} -- rustc --print sysroot > \"$RUNNER_TEMP/mbx-rust-sysroot\" && IFS= read -r sysroot < \"$RUNNER_TEMP/mbx-rust-sysroot\" && test -n \"$sysroot\" && printf '%s/bin\\n' \"$sysroot\" >> \"$GITHUB_PATH\""
            ),
        ],
        qualification_shell_env(request),
    )
}

fn mbx_action_step(request: &MbxQualificationPins, writer: bool) -> Result<Step, RenderError> {
    steps::action_step_with_env(
        MBX_SETUP_NAME,
        &request.mbx_action_uses,
        BTreeMap::from([
            ("backend".to_owned(), "local".to_owned()),
            (
                "velnor-cache-scope".to_owned(),
                QUALIFICATION_CACHE_SCOPE.to_owned(),
            ),
            ("velnor-cache-writer".to_owned(), writer.to_string()),
            ("version".to_owned(), request.mbx_version.clone()),
        ]),
        BTreeMap::new(),
    )
}

fn verify_action_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    shell_step(
        request,
        "Verify pinned MBX version",
        &format!(
            "mbx --version > \"$RUNNER_TEMP/mbx-version\" && grep -Fq '{}' \"$RUNNER_TEMP/mbx-version\"",
            request.mbx_version
        ),
    )
}

fn shell_step(
    request: &MbxQualificationPins,
    name: &str,
    command: &str,
) -> Result<Step, RenderError> {
    steps::shell_step(
        name,
        vec!["bash".to_owned(), "-c".to_owned(), command.to_owned()],
        qualification_shell_env(request),
    )
}

fn gate_writer_steps(steps: &mut [Step]) {
    let export_gate = format!(
        "success() && {QUALIFICATION_GATE} && steps.mbx-bundle.outputs.cache-hit != 'true'"
    );
    let save_gate = format!("{export_gate} && steps.mbx-export.outputs.ready == 'true'");
    for step in steps {
        match step.name.as_str() {
            mbx_bundle::MBX_BUNDLE_EXPORT_NAME => step.condition = Some(export_gate.clone()),
            mbx_bundle::MBX_BUNDLE_SAVE_NAME => step.condition = Some(save_gate.clone()),
            _ => {}
        }
    }
}

pub(super) fn qualification_shell_env(request: &MbxQualificationPins) -> BTreeMap<String, String> {
    let home = "${{ runner.temp }}/velnor-mbx-cache-qualification";
    BTreeMap::from([
        ("CARGO_HOME".to_owned(), format!("{home}/cargo")),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_CARGO_HOME".to_owned(), format!("{home}/cargo")),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_RUSTUP_HOME".to_owned(), format!("{home}/rustup")),
        ("RUSTUP_HOME".to_owned(), format!("{home}/rustup")),
        ("RUSTUP_TOOLCHAIN".to_owned(), request.rust_version.clone()),
    ])
}

pub(super) fn step_context() -> RenderContext {
    RenderContext {
        generator_version: "0.0.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.0.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor".to_owned(),
        checkout_uses: super::features::CHECKOUT_USES.to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        pull_request_cache_policy: PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn validate_exact_version(version: &str, kind: &str) -> Result<(), RenderError> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!(
            "bad_{kind}_version:{version}"
        )))
    }
}

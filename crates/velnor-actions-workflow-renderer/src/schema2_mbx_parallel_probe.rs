//! Hosted MBX parallel restore and independent-key writer qualification.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, PullRequestCachePolicy, Step,
};

use super::MbxQualificationPins;
use super::features::{base, finish, gated};
use crate::render::RenderContext;
use crate::steps::{self, MBX_SETUP_NAME};
use crate::yaml::Yaml;
use crate::{RenderError, mbx_bundle};
use helpers::{permission_yaml, qualification_job_env, step_context, validate_pins};

#[path = "schema2_mbx_parallel_probe_api.rs"]
mod api;
#[path = "schema2_mbx_parallel_probe_helpers.rs"]
mod helpers;

const PARALLEL_GATE: &str = "inputs.mode == 'mbx-cache-parallel' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true";
const SHARED_SCOPE: &str = "qualification-mbx-v1/parallel/shared";
const NEW_KEY_SCOPE: &str = "qualification-mbx-v1/parallel/new-key";
const COMPILE_STEP_NAME: &str = "Compile MBX cache probe";
const REUSE_STEP_NAME: &str = "Require reused compilation";
const IMPORT_PROBE: &str = "mbx cache stats --json | jq -e '.objects > 0' >/dev/null";
const REUSE_PROBE: &str = "mbx stats --json | jq -e '.savings.cached_compilations > 0' >/dev/null";
const SMOKE_CRATE: &str = r#"set -eu; root="$GITHUB_WORKSPACE/.velnor-mbx-parallel-qualification"; mkdir -p "$root/src"; printf '[package]\nname = "mbx-parallel-qualification"\nversion = "0.1.0"\nedition = "2024"\n\n[workspace]\nmembers = ["."]\nresolver = "3"\n\n[lib]\npath = "src/lib.rs"\n' > "$root/Cargo.toml"; printf 'pub fn cache_probe() -> u64 { 42 }\n' > "$root/src/lib.rs"; mbx build --manifest-path "$root/Cargo.toml""#;

/// Build parallel qualification jobs from pinned inputs and the scoped writer contract.
///
/// # Errors
/// Invalid action, Mise, MBX, Rust, hosted-runner, or writer-role inputs fail closed.
pub(super) fn jobs(
    request: &MbxQualificationPins,
    hosted: &Yaml,
) -> Result<Vec<(String, Yaml)>, RenderError> {
    validate_pins(request)?;
    let mut typed_jobs = BTreeMap::from([
        (
            Role::Seed.id().to_owned(),
            typed_job(request, hosted, Role::Seed)?,
        ),
        (
            Role::ReaderA.id().to_owned(),
            typed_job(request, hosted, Role::ReaderA)?,
        ),
        (
            Role::ReaderB.id().to_owned(),
            typed_job(request, hosted, Role::ReaderB)?,
        ),
        (
            Role::NewKeyWriter.id().to_owned(),
            typed_job(request, hosted, Role::NewKeyWriter)?,
        ),
        (
            Role::ObserverShared.id().to_owned(),
            typed_job(request, hosted, Role::ObserverShared)?,
        ),
        (
            Role::ObserverNew.id().to_owned(),
            typed_job(request, hosted, Role::ObserverNew)?,
        ),
    ]);
    mbx_bundle::append_single_bundle_saves(&mut typed_jobs, PullRequestCachePolicy::ReadOnly)?;
    super::mbx_resource_probe::attach(&mut typed_jobs, request)?;
    typed_jobs
        .into_iter()
        .map(|(id, job)| {
            let role = Role::from_id(&id).ok_or_else(|| {
                RenderError::InvalidWorkflow(format!("unknown_mbx_parallel_role:{id}"))
            })?;
            let mut job = job;
            if role.writer() {
                gate_writer_steps(&mut job.steps);
            }
            render_job(&id, job, request, role)
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Seed,
    ReaderA,
    ReaderB,
    NewKeyWriter,
    ObserverShared,
    ObserverNew,
}

impl Role {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "mbx-parallel-seed" => Some(Self::Seed),
            "mbx-parallel-reader-a" => Some(Self::ReaderA),
            "mbx-parallel-reader-b" => Some(Self::ReaderB),
            "mbx-parallel-new-key-writer" => Some(Self::NewKeyWriter),
            "mbx-parallel-observer-shared" => Some(Self::ObserverShared),
            "mbx-parallel-observer-new" => Some(Self::ObserverNew),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Seed => "mbx-parallel-seed",
            Self::ReaderA => "mbx-parallel-reader-a",
            Self::ReaderB => "mbx-parallel-reader-b",
            Self::NewKeyWriter => "mbx-parallel-new-key-writer",
            Self::ObserverShared => "mbx-parallel-observer-shared",
            Self::ObserverNew => "mbx-parallel-observer-new",
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Seed => "MBX parallel / seed",
            Self::ReaderA => "MBX parallel / reader-a",
            Self::ReaderB => "MBX parallel / reader-b",
            Self::NewKeyWriter => "MBX parallel / new-key-writer",
            Self::ObserverShared => "MBX parallel / observer-shared",
            Self::ObserverNew => "MBX parallel / observer-new",
        }
    }

    fn scope(self) -> &'static str {
        match self {
            Self::Seed | Self::ReaderA | Self::ReaderB | Self::ObserverShared => SHARED_SCOPE,
            Self::NewKeyWriter | Self::ObserverNew => NEW_KEY_SCOPE,
        }
    }

    fn writer(self) -> bool {
        matches!(self, Self::Seed | Self::NewKeyWriter)
    }

    fn needs(self) -> Vec<String> {
        match self {
            Self::Seed => Vec::new(),
            Self::ReaderA | Self::ReaderB | Self::NewKeyWriter => {
                vec![Role::Seed.id().to_owned()]
            }
            Self::ObserverShared | Self::ObserverNew => {
                [Role::Seed, Role::ReaderA, Role::ReaderB, Role::NewKeyWriter]
                    .into_iter()
                    .map(|role| role.id().to_owned())
                    .collect()
            }
        }
    }
}

fn typed_job(
    request: &MbxQualificationPins,
    hosted: &Yaml,
    role: Role,
) -> Result<Job, RenderError> {
    let runs_on = match hosted {
        Yaml::Str(label) => label.clone(),
        _ => {
            return Err(RenderError::InvalidWorkflow(
                "mbx_parallel_requires_hosted_runner".to_owned(),
            ));
        }
    };
    let timeout =
        JobTimeout::new(45).map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    let permissions = Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: if role.writer() {
            PermissionLevel::Write
        } else {
            PermissionLevel::Read
        },
    };
    let steps = qualification_steps(request, role)?;
    Ok(Job {
        display_name: role.display_name().to_owned(),
        runs_on,
        timeout_minutes: timeout,
        needs: role.needs(),
        condition: Some(PARALLEL_GATE.to_owned()),
        permissions: Some(permissions),
        environment: None,
        steps,
    })
}

fn qualification_steps(
    request: &MbxQualificationPins,
    role: Role,
) -> Result<Vec<Step>, RenderError> {
    let mut steps = Vec::new();
    if role == Role::ObserverShared {
        steps.extend(api::receipt_steps()?);
    }
    steps.extend([
        steps::checkout_step(super::features::CHECKOUT_USES)?,
        mise_setup_step(request)?,
        mise_install_step(request)?,
        shell_step(
            request,
            "Verify cache compression support",
            "command -v zstd >/dev/null && zstd --version >/dev/null && tar --version | grep -Fq 'GNU tar'",
        )?,
        mbx_action_step(request, role)?,
        shell_step(
            request,
            "Verify pinned MBX version",
            &format!(
                "mbx --version > \"$RUNNER_TEMP/mbx-version\" && grep -Fq '{}' \"$RUNNER_TEMP/mbx-version\"",
                request.mbx_version
            ),
        )?,
    ]);
    if !role.writer() {
        steps.push(shell_step(
            request,
            "Require imported MBX objects",
            IMPORT_PROBE,
        )?);
    }
    steps.push(shell_step(request, COMPILE_STEP_NAME, SMOKE_CRATE)?);
    if !role.writer() {
        steps.push(shell_step(
            request,
            "Require reused compilation",
            REUSE_PROBE,
        )?);
    }
    Ok(steps)
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
        super::mbx_qualification::qualification_shell_env(request),
    )
}

fn mbx_action_step(request: &MbxQualificationPins, role: Role) -> Result<Step, RenderError> {
    steps::action_step_with_env(
        MBX_SETUP_NAME,
        &request.mbx_action_uses,
        BTreeMap::from([
            ("backend".to_owned(), "local".to_owned()),
            ("velnor-cache-scope".to_owned(), role.scope().to_owned()),
            ("velnor-cache-writer".to_owned(), role.writer().to_string()),
            ("version".to_owned(), request.mbx_version.clone()),
        ]),
        BTreeMap::new(),
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
        super::mbx_qualification::qualification_shell_env(request),
    )
}

fn gate_writer_steps(steps: &mut [Step]) {
    let export_gate =
        format!("success() && {PARALLEL_GATE} && steps.mbx-bundle.outputs.cache-hit != 'true'");
    let save_gate = format!("{export_gate} && steps.mbx-export.outputs.ready == 'true'");
    for step in steps {
        match step.name.as_str() {
            mbx_bundle::MBX_BUNDLE_EXPORT_NAME => step.condition = Some(export_gate.clone()),
            mbx_bundle::MBX_BUNDLE_SAVE_NAME => step.condition = Some(save_gate.clone()),
            _ => {}
        }
    }
}

fn render_job(
    id: &str,
    mut job: Job,
    request: &MbxQualificationPins,
    role: Role,
) -> Result<(String, Yaml), RenderError> {
    let context = step_context();
    let steps = render_steps(id, &job.steps, &context, request, role)?;
    let mut fields = base(&job.display_name, Yaml::str(job.runs_on.clone()), 45);
    if !job.needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(job.needs.drain(..).map(Yaml::str).collect()),
        ));
    }
    fields.push(("permissions".to_owned(), permission_yaml(job.permissions)));
    fields.push(("env".to_owned(), qualification_job_env(request, role)));
    Ok(gated(finish(id, fields, steps), PARALLEL_GATE))
}

fn render_steps(
    id: &str,
    source: &[Step],
    context: &RenderContext,
    request: &MbxQualificationPins,
    role: Role,
) -> Result<Vec<Yaml>, RenderError> {
    let mut rendered =
        super::mbx_resource_probe_render::render_job_steps(id, source, request, context)?;
    if role != Role::ObserverShared {
        return Ok(rendered);
    }
    let mut insertion = None;
    for (index, step) in rendered.iter().enumerate() {
        match rendered_step_name(step) {
            Some(name) if name == REUSE_STEP_NAME => {
                if insertion.replace(index).is_some() {
                    return Err(RenderError::InvalidWorkflow(
                    "duplicate_mbx_parallel_reuse_probe".to_owned(),
                    ));
                }
            }
            Some(name) if name == api::API_STEP_NAME => {
                return Err(RenderError::InvalidWorkflow(
                    "duplicate_mbx_parallel_api_observer".to_owned(),
                ));
            }
            _ => {}
        }
    }
    let Some(index) = insertion else {
        return Err(RenderError::InvalidWorkflow(
                    "missing_mbx_parallel_reuse_probe".to_owned(),
        ));
    };
    rendered.insert(index + 1, api::observer_api_step(request));
    Ok(rendered)
}

fn rendered_step_name(step: &Yaml) -> Option<&str> {
    let Yaml::Map(fields) = step else {
        return None;
    };
    fields.iter().find_map(|(key, value)| {
        if key != "name" {
            return None;
        }
        match value {
            Yaml::Str(name) => Some(name.as_str()),
            _ => None,
        }
    })
}

#[cfg(test)]
#[path = "schema2_mbx_parallel_probe_tests.rs"]
mod tests;

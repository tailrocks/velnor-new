//! Hosted-only MBX single-bundle cache qualification.
//!
//! The writer and reader use the same production restore/import/export/save
//! graph. The writer is restricted to a protected-main workflow dispatch;
//! the dependent reader gets a separate hosted job and read-only permissions.

use std::collections::BTreeMap;

use super::MbxQualificationPins;
use super::features::{base, finish, gated};
use crate::cache_steps::MBX_ACTION_NAME;
use crate::render::RenderContext;
use crate::steps::{self, MBX_RESTORE_NAME};
use crate::yaml::Yaml;
use crate::{RenderError, document, mbx_bundle};
use velnor_actions_contract::{Job, JobTimeout, PermissionLevel, Permissions, Step};

const QUALIFICATION_GATE: &str = "inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true";
const SMOKE_CRATE: &str = r#"set -eu; root="$GITHUB_WORKSPACE/.velnor-mbx-cache-qualification"; mkdir -p "$root/src"; printf '[package]\nname = "mbx-cache-qualification"\nversion = "0.1.0"\nedition = "2024"\n\n[workspace]\nmembers = ["."]\nresolver = "3"\n\n[lib]\npath = "src/lib.rs"\n' > "$root/Cargo.toml"; printf 'pub fn cache_probe() -> u64 { 42 }\n' > "$root/src/lib.rs"; mbx build --manifest-path "$root/Cargo.toml""#;
const IMPORT_PROBE: &str = "mbx cache stats --json | jq -e '.objects > 0' >/dev/null";
const REUSE_PROBE: &str = "mbx stats --json | jq -e '.savings.cached_compilations > 0' >/dev/null";

/// Emit isolated writer and reader jobs for the pinned MBX runtime.
///
/// # Errors
/// Invalid action, Mise, MBX, or Rust pins fail closed.
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

    Ok(vec![
        job(request, hosted, true)?,
        job(request, hosted, false)?,
    ])
}

fn job(
    request: &MbxQualificationPins,
    hosted: &Yaml,
    writer: bool,
) -> Result<(String, Yaml), RenderError> {
    let (id, title, needs) = if writer {
        (
            "mbx-cache-write-hosted",
            "MBX objects cache / protected-main writer",
            Vec::new(),
        )
    } else {
        (
            "mbx-cache-read-hosted",
            "MBX objects cache / read-only reuse",
            vec!["mbx-cache-write-hosted".to_owned()],
        )
    };
    let steps = qualification_steps(request, writer)?;
    let permissions = Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: if writer {
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
    let typed_job = Job {
        display_name: title.to_owned(),
        runs_on,
        timeout_minutes: timeout,
        needs: needs.clone(),
        condition: Some(QUALIFICATION_GATE.to_owned()),
        permissions: Some(permissions),
        environment: None,
        steps,
    };
    let mut jobs = BTreeMap::from([(id.to_owned(), typed_job)]);
    mbx_bundle::append_single_bundle_saves(&mut jobs)?;
    let Some(mut typed_job) = jobs.remove(id) else {
        return Err(RenderError::InvalidWorkflow(format!(
            "missing_mbx_job:{id}"
        )));
    };
    if writer {
        gate_writer_steps(&mut typed_job.steps);
    } else {
        typed_job.steps.retain(|step| {
            step.name != mbx_bundle::MBX_BUNDLE_EXPORT_NAME
                && step.name != mbx_bundle::MBX_BUNDLE_SAVE_NAME
        });
    }
    let rendered_steps = typed_job
        .steps
        .iter()
        .map(|step| document::step_to_yaml(id, step, &step_context(), &[], false))
        .collect::<Result<Vec<_>, _>>()?;

    let mut fields = base(title, hosted.clone(), 45);
    if !needs.is_empty() {
        fields.push((
            "needs".to_owned(),
            Yaml::Seq(needs.iter().cloned().map(Yaml::str).collect()),
        ));
    }
    fields.push(("permissions".to_owned(), permission_yaml(writer)));
    fields.push(("env".to_owned(), qualification_env(request)));
    Ok(gated(
        finish(id, fields, rendered_steps),
        QUALIFICATION_GATE,
    ))
}

fn qualification_steps(
    request: &MbxQualificationPins,
    writer: bool,
) -> Result<Vec<Step>, RenderError> {
    let mut steps = vec![
        checkout_step()?,
        mise_setup_step(request)?,
        mise_install_step(request)?,
        mbx_action_step(request)?,
        verify_action_step(request)?,
    ];
    if !writer {
        steps.push(shell_step("Require imported MBX objects", IMPORT_PROBE)?);
    }
    steps.push(shell_step("Compile MBX cache probe", SMOKE_CRATE)?);
    if !writer {
        steps.push(shell_step("Require reused compilation", REUSE_PROBE)?);
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
    shell_step(
        "Install pinned Rust toolchain",
        &format!(
            "mise install rust@{rust} && mise exec rust@{rust} -- rustc --print sysroot > \"$RUNNER_TEMP/mbx-rust-sysroot\" && IFS= read -r sysroot < \"$RUNNER_TEMP/mbx-rust-sysroot\" && test -n \"$sysroot\" && printf '%s/bin\\n' \"$sysroot\" >> \"$GITHUB_PATH\""
        ),
    )
}

fn mbx_action_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    steps::action_step_with_env(
        MBX_RESTORE_NAME,
        &request.mbx_action_uses,
        BTreeMap::from([
            ("backend".to_owned(), "local".to_owned()),
            ("version".to_owned(), request.mbx_version.clone()),
        ]),
        BTreeMap::from([("ACTIONS_CACHE_MODE".to_owned(), "read".to_owned())]),
    )
}

fn verify_action_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    shell_step(
        "Verify pinned MBX version",
        &format!(
            "mbx --version > \"$RUNNER_TEMP/mbx-version\" && grep -Fq '{}' \"$RUNNER_TEMP/mbx-version\"",
            request.mbx_version
        ),
    )
}

fn shell_step(name: &str, command: &str) -> Result<Step, RenderError> {
    steps::shell_step(
        name,
        vec!["bash".to_owned(), "-c".to_owned(), command.to_owned()],
        BTreeMap::new(),
    )
}

fn gate_writer_steps(steps: &mut [Step]) {
    let export_gate = format!(
        "success() && {QUALIFICATION_GATE} && steps.mbx.outputs.cache-hit != 'true' && steps.mbx-bundle.outputs.cache-hit != 'true'"
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

fn permission_yaml(writer: bool) -> Yaml {
    mapping(&[
        ("contents", "read"),
        ("actions", if writer { "write" } else { "read" }),
    ])
}

fn qualification_env(request: &MbxQualificationPins) -> Yaml {
    let home = "${{ github.workspace }}/.velnor-mbx-cache-qualification";
    let export_group = format!(
        "velnor-qualification-mbx-{}-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-${{{{ github.sha }}}}",
        request.mbx_version
    );
    mapping(&[
        ("ACTIONS_CACHE_MODE", "read"),
        ("CARGO_HOME", &format!("{home}/cargo")),
        ("MBX_CACHE_DIR", &format!("{home}/mbx-store")),
        ("MBX_CACHE_EXPORT_GROUP", &export_group),
        ("MBX_GC_AUTO", "1"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_CARGO_HOME", &format!("{home}/cargo")),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_RUSTUP_HOME", &format!("{home}/rustup")),
        ("RUSTUP_HOME", &format!("{home}/rustup")),
        ("RUSTUP_TOOLCHAIN", &request.rust_version),
    ])
}

fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

fn step_context() -> RenderContext {
    RenderContext {
        generator_version: "0.0.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.0.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor".to_owned(),
        checkout_uses: super::features::CHECKOUT_USES.to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
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

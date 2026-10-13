//! Fixed-step construction for the hosted MBX cancellation probe.

use std::collections::BTreeMap;

#[path = "schema2_mbx_cancel_probe_util.rs"]
mod util;
use util::{
    argv, phase_env, rust_env as util_rust_env, upload_artifact_step, validate_exact_version,
    victim_phase_env,
};

use super::MbxQualificationPins;
use super::Phase;
use super::scripts;
use crate::RenderError;
use crate::cache_steps::MBX_ACTION_NAME;
use crate::steps::{self, MBX_SETUP_NAME};
use crate::yaml::Yaml;
use velnor_actions_contract::{Job, JobTimeout, PermissionLevel, Permissions, Step};

#[path = "schema2_mbx_cancel_probe_observer_steps.rs"]
mod observer_steps;

const HOSTED_RUNNER: &str = "ubuntu-26.04";
const CONTROLLER_RECEIPT_PATH: &str = "${{ runner.temp }}/mbx-cancel/receipt.json";
const OBSERVER_RECEIPT_DIR: &str = "${{ runner.temp }}/mbx-cancel/controller";
const OBSERVER_RESULT_PATH: &str = "${{ runner.temp }}/mbx-cancel/observer/result.json";
const VICTIM_RECEIPT_PATH: &str = "${{ runner.temp }}/mbx-cancel/victim/readiness.json";

pub(super) fn validate_request(
    request: &MbxQualificationPins,
    hosted: &Yaml,
) -> Result<(), RenderError> {
    request.mise_setup.validate()?;
    steps::validate_uses(&request.mbx_action_uses)?;
    if !request
        .mbx_action_uses
        .starts_with(&format!("{MBX_ACTION_NAME}@"))
    {
        return Err(RenderError::BadActionRef(format!(
            "not_mbx_action:{}",
            request.mbx_action_uses
        )));
    }
    validate_exact_version(&request.mbx_version, "mbx")?;
    validate_exact_version(&request.rust_version, "rust")?;
    if hosted != &Yaml::str(HOSTED_RUNNER) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_requires_ubuntu_26_04".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn rust_env(request: &MbxQualificationPins) -> BTreeMap<String, String> {
    util_rust_env(request)
}

pub(super) fn observer_cache_before_step(
    request: &MbxQualificationPins,
    phase: Phase,
) -> Result<Yaml, RenderError> {
    observer_steps::observer_cache_before_step(request, phase)
}

pub(super) fn observer_evidence_step(
    request: &MbxQualificationPins,
    phase: Phase,
) -> Result<Yaml, RenderError> {
    observer_steps::observer_evidence_step(request, phase)
}

pub(super) fn observer_measure_import_step() -> Yaml {
    observer_steps::observer_measure_import_step()
}

pub(super) fn observer_measure_reuse_step() -> Yaml {
    observer_steps::observer_measure_reuse_step()
}

pub(super) fn observer_classify_step(request: &MbxQualificationPins, phase: Phase) -> Yaml {
    observer_steps::observer_classify_step(request, phase)
}

pub(super) fn make_job(
    title: String,
    hosted: &Yaml,
    gate: String,
    needs: Vec<String>,
    actions: PermissionLevel,
    steps: Vec<Step>,
    timeout_minutes: u16,
) -> Result<Job, RenderError> {
    let Yaml::Str(runs_on) = hosted else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_cancel_requires_hosted_runner".to_owned(),
        ));
    };
    let timeout = JobTimeout::new(timeout_minutes)
        .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?;
    Ok(Job {
        display_name: title,
        runs_on: runs_on.clone(),
        timeout_minutes: timeout,
        needs,
        condition: Some(gate),
        permissions: Some(Permissions {
            contents: PermissionLevel::None,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions,
        }),
        environment: None,
        steps,
    })
}

pub(super) fn victim_identity_yaml(request: &MbxQualificationPins, phase: Phase) -> Yaml {
    super::render::bash_step(
        "Validate MBX cancellation victim identity",
        None,
        scripts::VICTIM_IDENTITY,
        &victim_phase_env(request, phase),
    )
}

pub(super) fn mise_setup_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
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

pub(super) fn mise_install_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    let command = format!(
        "mise install rust@{0} && mise exec rust@{0} -- rustc --print sysroot > \"$RUNNER_TEMP/mbx-cancel-sysroot\" && IFS= read -r sysroot < \"$RUNNER_TEMP/mbx-cancel-sysroot\" && test -n \"$sysroot\" && printf '%s/bin\\n' \"$sysroot\" >> \"$GITHUB_PATH\"",
        request.rust_version
    );
    steps::shell_step(
        "Install pinned Rust toolchain",
        argv(&command),
        rust_env(request),
    )
}

pub(super) fn mbx_action_step(
    request: &MbxQualificationPins,
    phase: Phase,
    writer: bool,
) -> Result<Step, RenderError> {
    steps::action_step(
        MBX_SETUP_NAME,
        &request.mbx_action_uses,
        BTreeMap::from([
            ("backend".to_owned(), "local".to_owned()),
            ("velnor-cache-scope".to_owned(), phase.scope().to_owned()),
            ("velnor-cache-writer".to_owned(), writer.to_string()),
            ("version".to_owned(), request.mbx_version.clone()),
        ]),
    )
}

pub(super) fn verify_action_step(request: &MbxQualificationPins) -> Result<Step, RenderError> {
    steps::shell_step(
        "Verify pinned MBX version",
        argv(&format!(
            "mbx --version > \"$RUNNER_TEMP/mbx-version\" && grep -Fq '{}' \"$RUNNER_TEMP/mbx-version\"",
            request.mbx_version
        )),
        rust_env(request),
    )
}

pub(super) fn source_fetch_yaml(condition: Option<&str>) -> Yaml {
    super::render::bash_step_if(
        "Fetch exact protected-main source",
        scripts::FETCH_SOURCE,
        &BTreeMap::new(),
        condition,
    )
}

pub(super) fn workspace_build_yaml(
    request: &MbxQualificationPins,
    condition: Option<&str>,
) -> Yaml {
    super::render::bash_step_if(
        "Build pinned MBX workspace",
        scripts::BUILD_WORKSPACE,
        &rust_env(request),
        condition,
    )
}

pub(super) fn no_write_guard_yaml() -> Yaml {
    super::render::bash_step(
        "Confirm pre-save victim has no writer payload",
        None,
        scripts::PRE_SAVE_GUARD,
        &BTreeMap::new(),
    )
}

pub(super) fn victim_receipt_yaml(request: &MbxQualificationPins, phase: Phase) -> Yaml {
    let mut env = victim_phase_env(request, phase);
    env.extend([
        (
            "MBX_PRIMARY".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
        ),
        (
            "MBX_GENERATION".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.generation }}".to_owned(),
        ),
        (
            "MBX_RUSTC_IDENTITY".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.rustc_identity }}".to_owned(),
        ),
        (
            "MBX_RESOLVED_VERSION".to_owned(),
            "${{ steps.mbx.outputs.mbx-version }}".to_owned(),
        ),
    ]);
    super::render::bash_step(
        "Write MBX cancellation readiness receipt",
        None,
        scripts::WRITE_VICTIM_RECEIPT,
        &env,
    )
}

pub(super) fn wait_before_save_yaml() -> Yaml {
    super::render::bash_step(
        "Wait at MBX pre-save cancellation point",
        None,
        scripts::PRE_SAVE_WAIT,
        &BTreeMap::new(),
    )
}

#[derive(Clone, Copy)]
pub(super) struct ApiShellSpec<'a> {
    pub name: &'a str,
    pub script: &'a str,
    pub request: &'a MbxQualificationPins,
    pub phase: Phase,
    pub probe_id: &'a str,
    pub child_run_id: Option<&'a str>,
    pub child_workflow_id: Option<&'a str>,
    pub id: Option<&'a str>,
    pub condition: Option<&'a str>,
}

pub(super) fn api_shell_step(spec: ApiShellSpec<'_>) -> Result<Yaml, RenderError> {
    let mut env = phase_env(spec.request, spec.phase);
    if !spec.probe_id.is_empty() {
        env.insert("PROBE_ID".to_owned(), spec.probe_id.to_owned());
    }
    if let Some(value) = spec.child_run_id {
        env.insert("RUN_ID".to_owned(), value.to_owned());
    }
    if let Some(value) = spec.child_workflow_id {
        env.insert("WORKFLOW_ID".to_owned(), value.to_owned());
    }
    super::render::token_bash_step(spec.name, spec.id, spec.script, &env, spec.condition)
}

pub(super) fn controller_receipt_yaml(request: &MbxQualificationPins, phase: Phase) -> Yaml {
    let mut env = phase_env(request, phase);
    env.extend([
        (
            "PROBE_ID".to_owned(),
            "${{ steps.cancel-probe-id.outputs.probe_id }}".to_owned(),
        ),
        (
            "RUN_ID".to_owned(),
            "${{ steps.cancel-dispatch.outputs.workflow_run_id }}".to_owned(),
        ),
        (
            "WORKFLOW_ID".to_owned(),
            "${{ steps.cancel-dispatch.outputs.workflow_id }}".to_owned(),
        ),
        (
            "RUN_URL".to_owned(),
            "${{ steps.cancel-dispatch.outputs.run_url }}".to_owned(),
        ),
        (
            "DISPATCH_STATUS".to_owned(),
            "${{ steps.cancel-dispatch.outputs.dispatch_status }}".to_owned(),
        ),
        (
            "READY".to_owned(),
            "${{ steps.cancel-ready.outputs.ready }}".to_owned(),
        ),
        (
            "READY_REASON".to_owned(),
            "${{ steps.cancel-ready.outputs.reason }}".to_owned(),
        ),
        (
            "CANCEL_REQUESTED".to_owned(),
            "${{ steps.cancel-request.outputs.cancel_requested }}".to_owned(),
        ),
        (
            "CANCEL_STATUS".to_owned(),
            "${{ steps.cancel-request.outputs.cancel_status }}".to_owned(),
        ),
        (
            "POST_REVALIDATED".to_owned(),
            "${{ steps.cancel-request.outputs.post_revalidated }}".to_owned(),
        ),
        (
            "CANCEL_REASON".to_owned(),
            "${{ steps.cancel-request.outputs.reason }}".to_owned(),
        ),
        (
            "TERMINAL".to_owned(),
            "${{ steps.cancel-terminal.outputs.terminal }}".to_owned(),
        ),
        (
            "TERMINAL_STATE".to_owned(),
            "${{ steps.cancel-terminal.outputs.terminal_state }}".to_owned(),
        ),
    ]);
    super::render::bash_step_if(
        "Write controller evidence receipt",
        scripts::CONTROLLER_RECEIPT,
        &env,
        Some("always()"),
    )
}

pub(super) fn upload_controller_receipt_step(phase: Phase) -> Result<Step, RenderError> {
    let artifact_name = format!("mbx-cancel-controller-receipt-{}", phase.token());
    let mut step = upload_artifact_step(&artifact_name, CONTROLLER_RECEIPT_PATH)?;
    step.condition = Some("always()".to_owned());
    Ok(step)
}

pub(super) fn upload_victim_receipt_step(phase: Phase) -> Result<Step, RenderError> {
    let name = match phase {
        Phase::PreSave => "mbx-cancel-victim-pre-save",
        Phase::DuringSave => "mbx-cancel-victim-during-save",
    };
    upload_artifact_step(name, VICTIM_RECEIPT_PATH)
}

pub(super) fn download_controller_receipt_step(phase: Phase) -> Result<Step, RenderError> {
    let artifact_name = format!("mbx-cancel-controller-receipt-{}", phase.token());
    steps::download_artifact_step(&artifact_name, OBSERVER_RECEIPT_DIR)
}

pub(super) fn upload_observer_receipt_step(phase: Phase) -> Result<Step, RenderError> {
    let name = match phase {
        Phase::PreSave => "mbx-cancel-observer-pre-save",
        Phase::DuringSave => "mbx-cancel-observer-during-save",
    };
    let mut step = upload_artifact_step(name, OBSERVER_RESULT_PATH)?;
    step.condition = Some("always()".to_owned());
    Ok(step)
}

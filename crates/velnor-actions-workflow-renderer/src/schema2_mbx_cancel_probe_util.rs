//! Shared fixed environment, pin checks, and receipt artifact step helpers.

use std::collections::BTreeMap;

use super::{MbxQualificationPins, Phase};
use crate::steps;
use crate::{RenderError, artifact_paths};
use velnor_actions_contract::Step;

pub(super) fn rust_env(request: &MbxQualificationPins) -> BTreeMap<String, String> {
    let root = "${{ runner.temp }}/velnor-mbx-cancel";
    BTreeMap::from([
        ("CARGO_HOME".to_owned(), format!("{root}/cargo")),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_CARGO_HOME".to_owned(), format!("{root}/cargo")),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_RUSTUP_HOME".to_owned(), format!("{root}/rustup")),
        ("RUSTUP_HOME".to_owned(), format!("{root}/rustup")),
        ("RUSTUP_TOOLCHAIN".to_owned(), request.rust_version.clone()),
    ])
}

pub(super) fn phase_env(request: &MbxQualificationPins, phase: Phase) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("PROBE_PHASE".to_owned(), phase.token().to_owned()),
        (
            "REF_PROTECTED".to_owned(),
            "${{ github.ref_protected }}".to_owned(),
        ),
        ("VICTIM_MODE".to_owned(), phase.victim_mode().to_owned()),
        (
            "CONTROLLER_MODE".to_owned(),
            phase.controller_mode().to_owned(),
        ),
        ("CACHE_SCOPE".to_owned(), phase.scope().to_owned()),
        (
            "VICTIM_JOB_NAME".to_owned(),
            format!("MBX cancellation / {} victim", phase.token()),
        ),
        (
            "VICTIM_ARTIFACT_NAME".to_owned(),
            format!("mbx-cancel-victim-{}", phase.token()),
        ),
        (
            "CANCEL_STEP_NAME".to_owned(),
            match phase {
                Phase::PreSave => "Wait at MBX pre-save cancellation point".to_owned(),
                Phase::DuringSave => crate::mbx_bundle::MBX_BUNDLE_SAVE_NAME.to_owned(),
            },
        ),
        (
            "MBX_ACTION_USES".to_owned(),
            request.mbx_action_uses.clone(),
        ),
        ("MBX_VERSION".to_owned(), request.mbx_version.clone()),
        (
            "MBX_GENERATION".to_owned(),
            velnor_actions_contract::cachekey::mbx_cache_generation(&request.mbx_version),
        ),
        ("RUST_VERSION".to_owned(), request.rust_version.clone()),
        (
            "MISE_ACTION_USES".to_owned(),
            request.mise_setup.uses.clone(),
        ),
        (
            "MISE_VERSION".to_owned(),
            request.mise_setup.version.clone(),
        ),
        ("MISE_SHA256".to_owned(), request.mise_setup.sha256.clone()),
        (
            "CACHE_RESTORE_USES".to_owned(),
            steps::TOOLS_RESTORE_USES.to_owned(),
        ),
        (
            "CACHE_SAVE_USES".to_owned(),
            steps::TOOLS_SAVE_USES.to_owned(),
        ),
        (
            "UPLOAD_ARTIFACT_USES".to_owned(),
            steps::UPLOAD_ARTIFACT_USES.to_owned(),
        ),
        (
            "DOWNLOAD_ARTIFACT_USES".to_owned(),
            steps::DOWNLOAD_ARTIFACT_USES.to_owned(),
        ),
    ])
}

pub(super) fn victim_phase_env(
    request: &MbxQualificationPins,
    phase: Phase,
) -> BTreeMap<String, String> {
    let mut env = phase_env(request, phase);
    env.insert("PROBE_ID".to_owned(), "${{ inputs.probe_id }}".to_owned());
    env
}

pub(super) fn controller_receipt_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "PROBE_ID".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.probe_id }}".to_owned(),
        ),
        (
            "CHILD_SOURCE_SHA".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.source_sha }}".to_owned(),
        ),
        (
            "CHILD_ACTOR".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.child_actor }}".to_owned(),
        ),
        (
            "VALIDATED_CACHE_KEY".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
        ),
        (
            "CONTROLLER_READY".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.ready }}".to_owned(),
        ),
        (
            "CONTROLLER_READY_REASON".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.ready_reason }}".to_owned(),
        ),
        (
            "CONTROLLER_CANCEL_REQUESTED".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.cancel_requested }}".to_owned(),
        ),
        (
            "CONTROLLER_CANCEL_STATUS".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.cancel_status }}".to_owned(),
        ),
        (
            "CONTROLLER_POST_REVALIDATED".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.post_revalidated }}".to_owned(),
        ),
        (
            "CONTROLLER_CANCEL_AT".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.cancel_request_started_at }}".to_owned(),
        ),
        (
            "CONTROLLER_TERMINAL".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.terminal }}".to_owned(),
        ),
        (
            "CONTROLLER_TERMINAL_STATE".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.terminal_state }}".to_owned(),
        ),
        (
            "CONTROLLER_BEFORE_COUNT".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.controller_cache_before_count }}".to_owned(),
        ),
    ])
}

pub(super) fn upload_artifact_step(name: &str, path: &str) -> Result<Step, RenderError> {
    if name.trim().is_empty() || path.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_artifact_io".to_owned()));
    }
    artifact_paths::check_artifact_path(path)?;
    steps::action_step(
        "Upload MBX cancellation receipt",
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), name.to_owned()),
            ("path".to_owned(), path.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("retention-days".to_owned(), "1".to_owned()),
        ]),
    )
}

pub(super) fn argv(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

pub(super) fn validate_exact_version(version: &str, kind: &str) -> Result<(), RenderError> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!(
            "bad_{kind}_version:{version}"
        )))
    }
}

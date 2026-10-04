//! Canonical victim receipt bodies for the exact controller fixtures.

use super::{
    MBX_ACTION, MISE_ACTION, MISE_SHA, PHASE, PRE_SCOPE_HASH, PROBE_ID, RUSTC_IDENTITY, SCOPE,
    SCOPE_HASH, SOURCE_SHA, VICTIM_MODE,
};

pub(in crate::schema2::mbx_cancel_probe::tests) fn readiness_receipt(pre_save: bool) -> String {
    let (mode, phase, scope, scope_hash) = if pre_save {
        (
            "mbx-cancel-pre-save-victim",
            "pre-save",
            "qualification-mbx-v1/cancel-pre-save-victim",
            PRE_SCOPE_HASH,
        )
    } else {
        (VICTIM_MODE, PHASE, SCOPE, SCOPE_HASH)
    };
    let cache_key = format!(
        "linux-x64-mbx-velnor-mbx-1.22.0-dir-rust-1.98.1-{RUSTC_IDENTITY}-scope-{scope_hash}-run-123-attempt-1-{SOURCE_SHA}"
    );
    format!(
        r#"{{"schema":1,"probe_id":"{PROBE_ID}","mode":"{mode}","phase":"{phase}","child_run_id":"123","child_attempt":"1","repository":"tailrocks/velnor-new","workflow_path":".github/workflows/qualification.yml","event":"workflow_dispatch","ref":"refs/heads/main","source_sha":"{SOURCE_SHA}","actor":"github-actions[bot]","mbx_action_uses":"{MBX_ACTION}","mbx_version":"1.22.0","mbx_resolved_version":"1.22.0","cache_scope":"{scope}","cache_key":"{cache_key}","generation":"velnor-mbx-1.22.0","rustc_identity":"{RUSTC_IDENTITY}","rust_version":"1.98.1","mise_action_uses":"{MISE_ACTION}","mise_version":"2025.9.5","mise_sha256":"{MISE_SHA}"}}"#
    )
}

pub(in crate::schema2::mbx_cancel_probe::tests) fn expected_key() -> String {
    format!(
        "linux-x64-mbx-velnor-mbx-1.22.0-dir-rust-1.98.1-{RUSTC_IDENTITY}-scope-{SCOPE_HASH}-run-123-attempt-1-{SOURCE_SHA}"
    )
}

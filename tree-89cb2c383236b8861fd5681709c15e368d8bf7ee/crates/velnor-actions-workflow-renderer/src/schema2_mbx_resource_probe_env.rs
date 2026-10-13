//! Exact environment bindings for the native hosted MBX evidence steps.

use std::collections::BTreeMap;

use super::MbxQualificationPins;
use crate::RenderError;

const EVIDENCE_EXPR: &str = "${{ runner.temp }}/mbx-cache-evidence";
const INTERVAL_SECONDS: &str = "5";

pub(super) fn validate_native_env(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    let mut validated = env.clone();
    for (key, value) in env {
        if native_output_binding(key, value) {
            validated.insert(key.clone(), "fixed qualified step output".to_owned());
        }
    }
    crate::commands::validate_env(&validated)
}

fn native_output_binding(key: &str, value: &str) -> bool {
    [
        (
            "MBX_SELECTED_CACHE_ROOT",
            "${{ steps.mbx-bundle-import.outputs.selected_cache_root }}",
        ),
        (
            "MATCHED",
            "${{ steps.mbx-bundle.outputs.cache-matched-key }}",
        ),
        ("PRIMARY", "${{ steps.mbx-bundle-key.outputs.primary }}"),
        ("CACHE_HIT", "${{ steps.mbx-bundle.outputs.cache-hit }}"),
        (
            "MBX_QUALIFICATION_CACHE_PRIMARY",
            "${{ steps.mbx-bundle-key.outputs.primary }}",
        ),
        (
            "MBX_QUALIFICATION_CACHE_PREFIX",
            "${{ steps.mbx-bundle-key.outputs.prefix }}",
        ),
        (
            "MBX_QUALIFICATION_CACHE_MATCHED_KEY",
            "${{ steps.mbx-bundle.outputs.cache-matched-key }}",
        ),
        (
            "MBX_QUALIFICATION_CACHE_HIT",
            "${{ steps.mbx-bundle.outputs.cache-hit }}",
        ),
        (
            "MBX_QUALIFICATION_RESTORE_PRIMARY_KEY",
            "${{ steps.mbx-bundle.outputs.cache-primary-key }}",
        ),
        (
            "MBX_QUALIFICATION_RESTORE_CONCLUSION",
            "${{ steps.mbx-bundle.conclusion }}",
        ),
        (
            "MBX_QUALIFICATION_EXPORT_READY",
            "${{ steps.mbx-export.outputs.ready }}",
        ),
        (
            "MBX_QUALIFICATION_EXPORT_STATUS",
            "${{ steps.mbx-export.outputs.export_status }}",
        ),
        (
            "MBX_QUALIFICATION_GC_STATUS",
            "${{ steps.mbx-export.outputs.gc_status }}",
        ),
        (
            "MBX_QUALIFICATION_SAVE_OUTCOME",
            "${{ steps.mbx-bundle-save.outcome }}",
        ),
        (
            "MBX_QUALIFICATION_CACHE_GENERATION",
            "${{ steps.mbx-bundle-key.outputs.generation }}",
        ),
        (
            "MBX_QUALIFICATION_RUSTC_IDENTITY",
            "${{ steps.mbx-bundle-key.outputs.rustc_identity }}",
        ),
    ]
    .contains(&(key, value))
}

pub(super) fn observer_env(
    request: &MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> BTreeMap<String, String> {
    let mut env = super::mbx_qualification::qualification_shell_env(request);
    let mut expected = vec!["restore-step-end", "import-step-end", "build-end", "final"];
    if writer {
        expected.extend([
            "export-complete",
            "gc-complete",
            "export-step-end",
            "save-step-end",
        ]);
    }
    if corrupt {
        expected.extend(["corruption-end", "corrupt-import-verified"]);
    }
    env.extend(BTreeMap::from([
        (
            "MBX_QUALIFICATION_PHASE_FILE".to_owned(),
            format!("{EVIDENCE_EXPR}/phases.tsv"),
        ),
        (
            "MBX_QUALIFICATION_SAMPLE_INTERVAL".to_owned(),
            INTERVAL_SECONDS.to_owned(),
        ),
        (
            "MBX_QUALIFICATION_FINALIZER_WAIT".to_owned(),
            "15".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_EXPECTED_INVENTORIES".to_owned(),
            expected.join(" "),
        ),
        ("MBX_QUALIFICATION_JOB_ID".to_owned(), job_id.to_owned()),
        ("MBX_QUALIFICATION_ROLE".to_owned(), role.to_owned()),
        (
            "MBX_SELECTED_CACHE_ROOT".to_owned(),
            "${{ steps.mbx-bundle-import.outputs.selected_cache_root }}".to_owned(),
        ),
    ]));
    env
}

pub(super) fn mutation_env(
    request: &MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> BTreeMap<String, String> {
    let mut env = observer_env(request, job_id, role, writer, corrupt);
    env.extend(BTreeMap::from([
        (
            "MATCHED".to_owned(),
            "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
        ),
        (
            "PRIMARY".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
        ),
        (
            "CACHE_HIT".to_owned(),
            "${{ steps.mbx-bundle.outputs.cache-hit }}".to_owned(),
        ),
    ]));
    env
}

pub(super) fn receipt_env(
    request: &MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> BTreeMap<String, String> {
    let mut env = observer_env(request, job_id, role, writer, corrupt);
    env.extend(BTreeMap::from([
        (
            "MBX_QUALIFICATION_CACHE_PRIMARY".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_CACHE_PREFIX".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.prefix }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_CACHE_MATCHED_KEY".to_owned(),
            "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_CACHE_HIT".to_owned(),
            "${{ steps.mbx-bundle.outputs.cache-hit }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_RESTORE_PRIMARY_KEY".to_owned(),
            "${{ steps.mbx-bundle.outputs.cache-primary-key }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_RESTORE_CONCLUSION".to_owned(),
            "${{ steps.mbx-bundle.conclusion }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_EXPORT_READY".to_owned(),
            "${{ steps.mbx-export.outputs.ready }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_EXPORT_STATUS".to_owned(),
            "${{ steps.mbx-export.outputs.export_status }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_GC_STATUS".to_owned(),
            "${{ steps.mbx-export.outputs.gc_status }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_SAVE_OUTCOME".to_owned(),
            "${{ steps.mbx-bundle-save.outcome }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_CACHE_GENERATION".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.generation }}".to_owned(),
        ),
        (
            "MBX_QUALIFICATION_RUSTC_IDENTITY".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.rustc_identity }}".to_owned(),
        ),
    ]));
    env
}

#[cfg(test)]
mod tests {
    use super::native_output_binding;

    #[test]
    fn native_receipt_bindings_accept_only_exact_step_outputs() {
        assert!(native_output_binding(
            "MBX_SELECTED_CACHE_ROOT",
            "${{ steps.mbx-bundle-import.outputs.selected_cache_root }}"
        ));
        assert!(native_output_binding(
            "MBX_QUALIFICATION_CACHE_PREFIX",
            "${{ steps.mbx-bundle-key.outputs.prefix }}"
        ));
        assert!(native_output_binding(
            "MBX_QUALIFICATION_RESTORE_PRIMARY_KEY",
            "${{ steps.mbx-bundle.outputs.cache-primary-key }}"
        ));
        assert!(native_output_binding(
            "MBX_QUALIFICATION_RESTORE_CONCLUSION",
            "${{ steps.mbx-bundle.conclusion }}"
        ));
        assert!(native_output_binding(
            "MBX_QUALIFICATION_RUSTC_IDENTITY",
            "${{ steps.mbx-bundle-key.outputs.rustc_identity }}"
        ));
        assert!(!native_output_binding(
            "MBX_QUALIFICATION_CACHE_PREFIX",
            "${{ steps.mbx-bundle-key.outputs.prefix-extra }}"
        ));
        assert!(!native_output_binding(
            "MBX_SELECTED_CACHE_ROOT",
            "${{ steps.mbx-bundle-import.outputs.selected_cache_root-extra }}"
        ));
        assert!(!native_output_binding(
            "MBX_QUALIFICATION_RESTORE_PRIMARY_KEY",
            "${{ steps.mbx-bundle.outputs.cache-primary-key-extra }}"
        ));
        assert!(!native_output_binding(
            "MBX_QUALIFICATION_RESTORE_CONCLUSION",
            "${{ steps.mbx-bundle.outcome }}"
        ));
        assert!(!native_output_binding(
            "OTHER",
            "${{ steps.mbx-bundle-key.outputs.primary }}"
        ));
    }
}

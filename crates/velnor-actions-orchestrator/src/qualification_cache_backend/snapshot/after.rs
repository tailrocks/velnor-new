//! Post-producer cache census, bound to the same dispatch as the BEFORE snapshot.

use std::path::Path;

use super::{
    MAX_SNAPSHOT_BYTES, QualificationCacheBackendSnapshot, QualificationDispatch,
    QualificationPhase, canonical_json_bytes, census, run_key_for_ci, validate,
};

pub(crate) const QUALIFICATION_CACHE_BACKEND_AFTER_FILENAME: &str =
    "qualification-cache-backend-after.json";

pub(crate) fn capture_qualification_cache_after(
    workspace: &Path,
    context: &QualificationDispatch,
) -> Result<QualificationCacheBackendSnapshot, crate::OrchestratorError> {
    if std::env::var_os("GH_TOKEN").is_none_or(|value| value.is_empty()) {
        return Err(crate::internal::internal(
            "qualification_cache_after_auth_missing",
        ));
    }
    let run_key = run_key_for_ci(context.run_id, u64::from(context.run_attempt));
    let snapshot = if context.phase == QualificationPhase::Control {
        QualificationCacheBackendSnapshot::new_for_stage(
            context,
            run_key,
            "not_queried",
            None,
            None,
            vec![],
            "after",
        )?
    } else {
        let started = census::unix_ms()?;
        let entries = census::query_ref_census(workspace, &context.repository, &context.git_ref)?;
        let finished = census::unix_ms()?;
        QualificationCacheBackendSnapshot::new_for_stage(
            context,
            run_key,
            "complete",
            Some(started),
            Some(finished),
            entries,
            "after",
        )?
    };
    Ok(snapshot)
}

pub(crate) fn parse_qualification_cache_after(
    bytes: &[u8],
    expected: &QualificationDispatch,
) -> Result<QualificationCacheBackendSnapshot, crate::OrchestratorError> {
    let value = super::parse_snapshot_value(bytes)?;
    super::parse_snapshot(value, bytes, expected, "after")
}

pub(crate) fn stage_qualification_cache_after(
    run_dir: &Path,
    snapshot: &QualificationCacheBackendSnapshot,
) -> Result<(), crate::OrchestratorError> {
    validate(snapshot, &snapshot.context, "after")?;
    if run_dir.file_name().and_then(std::ffi::OsStr::to_str) != Some(snapshot.run_key.as_str()) {
        return Err(crate::internal::internal(
            "qualification_cache_snapshot_run_mismatch",
        ));
    }
    let bytes = canonical_json_bytes(snapshot).map_err(crate::internal::internal_contract)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(crate::internal::internal(
            "qualification_cache_snapshot_bounds",
        ));
    }
    crate::exclusive_write::write_exclusive(
        &run_dir.join(QUALIFICATION_CACHE_BACKEND_AFTER_FILENAME),
        &bytes,
        "qualification_cache_after_snapshot",
    )
}

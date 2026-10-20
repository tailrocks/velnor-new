//! Canonical pre-plan cache census artifact.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::strict_json::parse_strict_json_bytes;
#[cfg(test)]
use velnor_actions_contract::workflow::QualificationCacheBackendObservation;
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationDispatch, QualificationPhase,
};
use velnor_actions_contract::{canonical_json_bytes, digest_b3, run_key_for_ci};

#[path = "snapshot/census.rs"]
mod census;
#[cfg(test)]
#[path = "snapshot/tests.rs"]
mod tests;

use super::CachePage;

const SNAPSHOT_SCHEMA: u32 = 1;
const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const QUALIFICATION_CACHE_BACKEND_BEFORE_FILENAME: &str =
    "qualification-cache-backend-before.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QualificationCacheBackendSnapshot {
    schema: u32,
    stage: String,
    context: QualificationDispatch,
    run_key: String,
    status: String,
    started_unix_ms: Option<u64>,
    finished_unix_ms: Option<u64>,
    entries: Vec<QualificationCacheBackendEntry>,
    digest: String,
}

#[derive(Serialize)]
struct SnapshotPayload<'a> {
    schema: u32,
    stage: &'static str,
    context: &'a QualificationDispatch,
    run_key: &'a str,
    status: &'a str,
    started_unix_ms: Option<u64>,
    finished_unix_ms: Option<u64>,
    entries: &'a [QualificationCacheBackendEntry],
}

impl QualificationCacheBackendSnapshot {
    fn payload(&self) -> SnapshotPayload<'_> {
        SnapshotPayload {
            schema: self.schema,
            stage: "before",
            context: &self.context,
            run_key: &self.run_key,
            status: &self.status,
            started_unix_ms: self.started_unix_ms,
            finished_unix_ms: self.finished_unix_ms,
            entries: &self.entries,
        }
    }

    fn new(
        context: &QualificationDispatch,
        run_key: String,
        status: &str,
        started_unix_ms: Option<u64>,
        finished_unix_ms: Option<u64>,
        entries: Vec<QualificationCacheBackendEntry>,
    ) -> Result<Self, crate::OrchestratorError> {
        let mut snapshot = Self {
            schema: SNAPSHOT_SCHEMA,
            stage: "before".to_owned(),
            context: context.clone(),
            run_key,
            status: status.to_owned(),
            started_unix_ms,
            finished_unix_ms,
            entries,
            digest: String::new(),
        };
        snapshot.digest = digest_b3(
            &canonical_json_bytes(&snapshot.payload())
                .map_err(crate::internal::internal_contract)?,
        );
        validate(&snapshot, context)?;
        Ok(snapshot)
    }

    #[cfg(test)]
    pub(crate) fn observation_for(
        &self,
        key: &str,
        git_ref: &str,
    ) -> Result<QualificationCacheBackendObservation, crate::OrchestratorError> {
        validate(self, &self.context)?;
        if git_ref != self.context.git_ref {
            return Err(crate::internal::internal(
                "qualification_cache_snapshot_ref_mismatch",
            ));
        }
        if self.status == "not_queried" {
            return Ok(QualificationCacheBackendObservation::NotQueried);
        }
        let mut found = self
            .entries
            .iter()
            .filter(|entry| entry.key == key && entry.git_ref == git_ref);
        let Some(entry) = found.next() else {
            return Ok(QualificationCacheBackendObservation::Absent);
        };
        if found.next().is_some() {
            return Err(crate::internal::internal(
                "qualification_cache_snapshot_ambiguous",
            ));
        }
        Ok(QualificationCacheBackendObservation::Found(entry.clone()))
    }

    pub(crate) fn validate_for(
        &self,
        context: &QualificationDispatch,
        run_key: &str,
    ) -> Result<(), crate::OrchestratorError> {
        validate(self, context)?;
        if self.run_key != run_key {
            return Err(crate::internal::internal(
                "qualification_cache_snapshot_run_mismatch",
            ));
        }
        Ok(())
    }
}

pub(crate) fn capture_qualification_cache_before(
    workspace: &Path,
    context: &QualificationDispatch,
) -> Result<QualificationCacheBackendSnapshot, crate::OrchestratorError> {
    let run_key = run_key_for_ci(context.run_id, u64::from(context.run_attempt));
    let snapshot = if context.phase == QualificationPhase::Control {
        QualificationCacheBackendSnapshot::new(context, run_key, "not_queried", None, None, vec![])?
    } else {
        let started = census::unix_ms()?;
        let entries = census::query_ref_census(workspace, &context.repository, &context.git_ref)?;
        let finished = census::unix_ms()?;
        QualificationCacheBackendSnapshot::new(
            context,
            run_key,
            "complete",
            Some(started),
            Some(finished),
            entries,
        )?
    };
    Ok(snapshot)
}

#[cfg(test)]
pub(crate) fn complete_snapshot_for_test(
    context: &QualificationDispatch,
) -> Result<QualificationCacheBackendSnapshot, crate::OrchestratorError> {
    QualificationCacheBackendSnapshot::new(
        context,
        run_key_for_ci(context.run_id, u64::from(context.run_attempt)),
        "complete",
        Some(1),
        Some(2),
        vec![],
    )
}

pub(crate) fn parse_qualification_cache_before(
    bytes: &[u8],
    expected: &QualificationDispatch,
) -> Result<QualificationCacheBackendSnapshot, crate::OrchestratorError> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(crate::internal::internal(
            "qualification_cache_snapshot_bounds",
        ));
    }
    let value = parse_strict_json_bytes(bytes, MAX_SNAPSHOT_BYTES)
        .map_err(|_| crate::internal::internal("qualification_cache_snapshot_invalid"))?;
    let snapshot: QualificationCacheBackendSnapshot = serde_json::from_value(value)
        .map_err(|_| crate::internal::internal("qualification_cache_snapshot_invalid"))?;
    let canonical = canonical_json_bytes(&snapshot).map_err(crate::internal::internal_contract)?;
    if canonical != bytes {
        return Err(crate::internal::internal(
            "qualification_cache_snapshot_noncanonical",
        ));
    }
    validate(&snapshot, expected)?;
    Ok(snapshot)
}

pub(crate) fn stage_qualification_cache_before(
    runner_temp: &Path,
    snapshot: &QualificationCacheBackendSnapshot,
) -> Result<(), crate::OrchestratorError> {
    let bytes = canonical_json_bytes(snapshot).map_err(crate::internal::internal_contract)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(crate::internal::internal(
            "qualification_cache_snapshot_bounds",
        ));
    }
    let path = snapshot_path(runner_temp, &snapshot.run_key);
    let parent = path
        .parent()
        .ok_or_else(|| crate::internal::internal("bad_qualification_snapshot_path"))?;
    crate::exclusive_write::create_dir_no_symlink(runner_temp, parent)?;
    crate::exclusive_write::write_exclusive(&path, &bytes, "qualification_cache_snapshot")
}

pub(crate) fn verify_staged_qualification_cache_before(
    dir: &Path,
    context: &QualificationDispatch,
    run_key: &str,
) -> Result<(), crate::OrchestratorError> {
    let path = dir.join(QUALIFICATION_CACHE_BACKEND_BEFORE_FILENAME);
    let text = crate::safe_read::read_event_file(&path, MAX_SNAPSHOT_BYTES as u64)?;
    let snapshot = parse_qualification_cache_before(text.as_bytes(), context)?;
    snapshot.validate_for(context, run_key)
}

pub(crate) fn snapshot_path(runner_temp: &Path, run_key: &str) -> std::path::PathBuf {
    runner_temp
        .join("velnor")
        .join(run_key)
        .join(QUALIFICATION_CACHE_BACKEND_BEFORE_FILENAME)
}

fn validate(
    snapshot: &QualificationCacheBackendSnapshot,
    expected: &QualificationDispatch,
) -> Result<(), crate::OrchestratorError> {
    expected
        .validate_shape()
        .map_err(crate::internal::internal_contract)?;
    if snapshot.schema != SNAPSHOT_SCHEMA
        || snapshot.stage != "before"
        || &snapshot.context != expected
        || snapshot.run_key != run_key_for_ci(expected.run_id, u64::from(expected.run_attempt))
    {
        return Err(crate::internal::internal(
            "qualification_cache_snapshot_binding",
        ));
    }
    let payload =
        canonical_json_bytes(&snapshot.payload()).map_err(crate::internal::internal_contract)?;
    if digest_b3(&payload) != snapshot.digest {
        return Err(crate::internal::internal(
            "qualification_cache_snapshot_digest",
        ));
    }
    match snapshot.status.as_str() {
        "not_queried"
            if expected.phase == QualificationPhase::Control
                && snapshot.started_unix_ms.is_none()
                && snapshot.finished_unix_ms.is_none()
                && snapshot.entries.is_empty() => {}
        "complete"
            if expected.phase != QualificationPhase::Control
                && snapshot.started_unix_ms.is_some_and(|time| time > 0)
                && snapshot
                    .finished_unix_ms
                    .zip(snapshot.started_unix_ms)
                    .is_some_and(|(finished, started)| finished >= started && finished > 0)
                && snapshot
                    .entries
                    .iter()
                    .all(|entry| entry.git_ref == expected.git_ref) =>
        {
            let mut sorted = snapshot.entries.clone();
            sort_entries(&mut sorted);
            if sorted != snapshot.entries {
                return Err(crate::internal::internal(
                    "qualification_cache_snapshot_order",
                ));
            }
            let mut ids = BTreeSet::new();
            if snapshot
                .entries
                .iter()
                .any(|entry| entry.id == 0 || !ids.insert(entry.id))
            {
                return Err(crate::internal::internal(
                    "qualification_cache_snapshot_duplicate_id",
                ));
            }
        }
        _ => {
            return Err(crate::internal::internal(
                "qualification_cache_snapshot_status",
            ));
        }
    }
    Ok(())
}

fn sort_entries(entries: &mut [QualificationCacheBackendEntry]) {
    entries.sort_by(|left, right| {
        (&left.key, &left.git_ref, &left.version, left.id).cmp(&(
            &right.key,
            &right.git_ref,
            &right.version,
            right.id,
        ))
    });
}

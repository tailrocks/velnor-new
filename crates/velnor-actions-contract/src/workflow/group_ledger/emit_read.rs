// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::workflow::{TaskExecutionManifestEntryV1, UnitOutcome, UnitResultV1};
use crate::{TaskReport, TaskStatus, parse_strict_json};

use crate::helper_error::HelperError;

use super::emit::JobStatus;

const MAX_REPORT_BYTES: u64 = 1 << 20;
const MAX_SIDECAR_BYTES: u64 = 64 << 10;

pub(super) fn read_staged_report(
    tasks_dir: &Path,
    report_id: &str,
) -> Result<Option<String>, HelperError> {
    let path = tasks_dir.join(format!("{report_id}.json"));
    match crate::staged_reads::read_staged_text(&path, MAX_REPORT_BYTES) {
        Ok(text) => Ok(Some(text)),
        Err("missing") => Ok(None),
        Err(kind) => Err(HelperError::internal(&format!(
            "{kind}_task_report:{report_id}"
        ))),
    }
}

pub(super) fn read_staged_sidecar(
    tasks_dir: &Path,
    report_id: &str,
) -> Result<Option<Vec<u8>>, HelperError> {
    let path = tasks_dir.join(format!(
        "{report_id}{}",
        crate::workflow::UNIT_OBSERVATION_SUFFIX
    ));
    match crate::staged_reads::read_staged_text(&path, MAX_SIDECAR_BYTES) {
        Ok(text) => Ok(Some(text.into_bytes())),
        Err("missing") => Ok(None),
        Err(kind) => Err(HelperError::internal(&format!(
            "{kind}_observation:{report_id}"
        ))),
    }
}

pub(super) fn unit_result_for_report(
    unit_id: &str,
    entry: &TaskExecutionManifestEntryV1,
    report_id: &str,
    staged: Option<&str>,
    job_status: JobStatus,
    finished_secs: u64,
) -> Result<UnitResultV1, HelperError> {
    let recipe_id =
        UnitResultV1::recipe_digest_from_manifest_entry(entry).map_err(HelperError::from)?;
    let argv_digest =
        UnitResultV1::argv_digest_from_manifest_entry(entry).map_err(HelperError::from)?;
    let effective_input_digest = UnitResultV1::effective_input_digest_from_manifest_entry(entry)
        .map_err(HelperError::from)?;
    let task_identity = unit_id.to_owned();
    let Some(text) = staged else {
        if job_status == JobStatus::Success {
            return Err(HelperError::internal(&format!(
                "missing_task_report:{report_id}"
            )));
        }
        let (started_at, finished_at) = window_for_duration(finished_secs, 0);
        let result = UnitResultV1 {
            unit_id: unit_id.to_owned(),
            recipe_id,
            task_identity,
            argv_digest,
            effective_input_digest,
            outcome: UnitOutcome::NeedsPrepare,
            exit_code: None,
            started_at,
            finished_at,
            report_digest: UnitResultV1::missing_report_digest(),
        };
        result.validate().map_err(HelperError::from)?;
        return Ok(result);
    };
    let value =
        parse_strict_json(text).map_err(|_| HelperError::internal("unparsable_task_report"))?;
    let report: TaskReport = serde_json::from_value(value)
        .map_err(|_| HelperError::internal("unparsable_task_report"))?;
    report.validate().map_err(HelperError::from)?;
    let (outcome, exit_code) = outcome_for_report(&report, job_status, report_id)?;
    let duration_secs = report.duration_ms.unwrap_or(0) / 1_000;
    let (started_at, finished_at) = window_for_duration(finished_secs, duration_secs);
    let report_canonical = crate::canonical_json_bytes(&report).map_err(HelperError::from)?;
    let report_digest = UnitResultV1::report_digest_from_canonical_bytes(&report_canonical);
    let result = UnitResultV1 {
        unit_id: unit_id.to_owned(),
        recipe_id,
        task_identity,
        argv_digest,
        effective_input_digest,
        outcome,
        exit_code,
        started_at,
        finished_at,
        report_digest,
    };
    result.validate().map_err(HelperError::from)?;
    Ok(result)
}

pub(super) fn outcome_for_report(
    report: &TaskReport,
    job_status: JobStatus,
    report_id: &str,
) -> Result<(UnitOutcome, Option<i32>), HelperError> {
    let mapped = match (report.status, report.exit_code) {
        (TaskStatus::Executed, 0) => (UnitOutcome::Executed, Some(0)),
        (TaskStatus::Executed | TaskStatus::Failed, code) => (UnitOutcome::Failed, Some(code)),
        (TaskStatus::Cancelled, _) => (UnitOutcome::Cancelled, None),
        (TaskStatus::Reused, _) => (UnitOutcome::CacheHit, None),
        (TaskStatus::NotSelected | TaskStatus::EmptyPartition, _) => {
            (UnitOutcome::NeedsPrepare, None)
        }
    };
    if job_status == JobStatus::Success
        && matches!(mapped.0, UnitOutcome::Failed | UnitOutcome::Cancelled)
    {
        return Err(HelperError::internal(&format!(
            "failed_unit_on_success:{report_id}"
        )));
    }
    Ok(mapped)
}

pub(super) fn window_for_duration(finished_secs: u64, duration_secs: u64) -> (String, String) {
    let started_secs = finished_secs.saturating_sub(duration_secs);
    (utc_rfc3339(started_secs), utc_rfc3339(finished_secs))
}
pub(super) fn utc_rfc3339(secs: u64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(i64::try_from(days).unwrap_or(0));
    let hour = rest / 3_600;
    let minute = rest % 3_600 / 60;
    let second = rest % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

pub(super) fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

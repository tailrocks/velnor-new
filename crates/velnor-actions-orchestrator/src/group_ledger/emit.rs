// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::workflow::{
    ExecutionBranch, JobBindingV1, TaskExecutionManifestEntryV1, TaskExecutionManifestV1,
    UnitLedgerV1, UnitOutcome, UnitResultV1,
};
use velnor_actions_contract::{
    Plan, TaskReport, TaskStatus, parse_strict_json, plan_json_bytes, task_report_id_for_task,
    validate_matrix_key,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};

pub const EMIT_GROUP_LEDGER_OP: &str = "emit-group-ledger-v1";

pub(super) const GROUP_LEDGER_FILENAME: &str = "group-ledger.json";

const MAX_PLAN_JSON_BYTES: u64 = 4 << 20;
const MAX_MANIFEST_BYTES: u64 = 4 << 20;
const MAX_REPORT_BYTES: u64 = 1 << 20;
const MAX_SIDECAR_BYTES: u64 = 64 << 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobStatus {
    Success,
    Failure,
    Cancelled,
}

#[derive(Debug, Clone)]
pub(crate) struct EmitEnv {
    pub(crate) job_key: String,
    pub(crate) run_id: u64,
    pub(crate) run_attempt: u64,
    pub(crate) event: String,
    pub(crate) source_sha: String,
    pub(crate) ref_name: String,
    pub(crate) repository: String,
    pub(crate) workflow_sha: String,
    pub(crate) matrix_key: String,
    pub(crate) job_status: JobStatus,
}

/// Read the tokenless emit environment and write the matrix job ledger.
///
/// Identity binds on the repository slug, `run_id`, `run_attempt`, and
/// `job_key`: the numeric `repository_id`/`job_id` are unresolvable without
/// a token, so emit records them as `0` (see `JobBindingV1`).
pub fn emit_group_ledger() -> Result<PathBuf, OrchestratorError> {
    let emit = emit_env_from_process()?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    let runner_temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_dir = Path::new(&runner_temp).join("velnor").join(&run_key);
    let checkout = env::current_dir().map_err(|err| OrchestratorError::io(".", err.to_string()))?;
    let manifest_path = checkout.join(velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH);
    emit_group_ledger_to(&runner_temp, &manifest_path, &run_dir, &run_key, &emit)
}

fn emit_env_from_process() -> Result<EmitEnv, OrchestratorError> {
    let job_status = match required_env("VELNOR_JOB_STATUS")?.as_str() {
        "success" => JobStatus::Success,
        "failure" => JobStatus::Failure,
        "cancelled" => JobStatus::Cancelled,
        _ => return Err(internal("bad_job_status")),
    };
    let run_id = required_env("GITHUB_RUN_ID")?
        .parse::<u64>()
        .map_err(|_| internal("bad_run_id"))?;
    if run_id == 0 {
        return Err(internal("bad_run_id"));
    }
    let run_attempt = required_env("GITHUB_RUN_ATTEMPT")?
        .parse::<u64>()
        .map_err(|_| internal("bad_run_attempt"))?;
    if run_attempt == 0 {
        return Err(internal("bad_run_attempt"));
    }
    let repository = required_env("GITHUB_REPOSITORY")?;
    if crate::origin::validate_repository_slug(&repository).is_none() {
        return Err(internal("bad_repository_slug"));
    }
    Ok(EmitEnv {
        job_key: required_env("GITHUB_JOB")?,
        run_id,
        run_attempt,
        event: required_env("GITHUB_EVENT_NAME")?,
        source_sha: required_env("GITHUB_SHA")?,
        ref_name: required_env("GITHUB_REF")?,
        repository,
        workflow_sha: required_env("GITHUB_WORKFLOW_SHA")?,
        matrix_key: required_env("VELNOR_MATRIX_KEY")?,
        job_status,
    })
}

fn required_env(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            internal(&format!(
                "missing_{}",
                name.to_ascii_lowercase().replace('-', "_")
            ))
        })
}

pub(crate) fn emit_group_ledger_to(
    anchor: &Path,
    manifest_path: &Path,
    run_dir: &Path,
    run_key: &str,
    emit: &EmitEnv,
) -> Result<PathBuf, OrchestratorError> {
    validate_matrix_key(&emit.matrix_key).map_err(internal_contract)?;
    let plan_text =
        crate::retrieve_reports::read_staged_text(&run_dir.join("plan.json"), MAX_PLAN_JSON_BYTES)
            .map_err(|kind| internal(&format!("{kind}_plan")))?;
    let plan_value = parse_strict_json(&plan_text).map_err(|_| internal("unparsable_plan"))?;
    let plan: Plan = serde_json::from_value(plan_value).map_err(|_| internal("unparsable_plan"))?;
    plan.validate().map_err(internal_contract)?;
    if plan.run_key != run_key {
        return Err(internal("ledger_run_mismatch"));
    }
    let manifest_text =
        crate::retrieve_reports::read_staged_text(manifest_path, MAX_MANIFEST_BYTES)
            .map_err(|kind| internal(&format!("{kind}_manifest")))?;
    let manifest = parse_manifest_document(&manifest_text)?;
    let (group_key, branch) = split_job_key(&emit.job_key)?;
    let plan_canonical = plan_json_bytes(&plan).map_err(internal_contract)?;
    let plan_digest = UnitLedgerV1::plan_digest_from_canonical_bytes(&plan_canonical);
    let group_digest =
        UnitLedgerV1::group_digest_for_group(group_key, &plan_digest).map_err(internal_contract)?;
    let finished_secs = clock_secs()?;
    let tasks_dir = run_dir.join(&emit.matrix_key).join("tasks");
    let (ordered_results, observation_sidecars) =
        build_ordered_results(&plan, &manifest, run_key, emit, &tasks_dir, finished_secs)?;
    let observation_refs: Vec<Option<&[u8]>> = observation_sidecars
        .iter()
        .map(|sidecar| sidecar.as_deref())
        .collect();
    let observation_digest = UnitLedgerV1::observation_digest_for_sidecars(&observation_refs)
        .map_err(internal_contract)?;
    let source_binding_digest =
        UnitLedgerV1::source_binding_digest_for_sidecars(&vec![None; ordered_results.len()])
            .map_err(internal_contract)?;
    let ledger = UnitLedgerV1 {
        schema: 1,
        job: JobBindingV1 {
            schema: 1,
            repository_id: 0,
            repository: emit.repository.clone(),
            run_id: emit.run_id,
            run_attempt: emit.run_attempt,
            job_id: 0,
            job_key: emit.job_key.clone(),
            event: emit.event.clone(),
            ref_name: emit.ref_name.clone(),
            workflow_sha: emit.workflow_sha.clone(),
            source_sha: emit.source_sha.clone(),
            plan_digest,
        },
        group_digest,
        execution_branch: branch,
        selected_unit_ids: ordered_results
            .iter()
            .map(|result| result.unit_id.clone())
            .collect(),
        ordered_results,
        observation_digest,
        source_binding_digest,
    };
    ledger.validate().map_err(internal_contract)?;
    let bytes = ledger.to_canonical_bytes().map_err(internal_contract)?;
    let out_dir = run_dir.join(&emit.matrix_key);
    crate::exclusive_write::create_dir_no_symlink(anchor, &out_dir)?;
    let out_path = out_dir.join(GROUP_LEDGER_FILENAME);
    crate::exclusive_write::write_exclusive(&out_path, &bytes, "group_ledger")?;
    Ok(out_path)
}

type OrderedUnits = (Vec<UnitResultV1>, Vec<Option<Vec<u8>>>);

fn build_ordered_results(
    plan: &Plan,
    manifest: &TaskExecutionManifestV1,
    run_key: &str,
    emit: &EmitEnv,
    tasks_dir: &Path,
    finished_secs: u64,
) -> Result<OrderedUnits, OrchestratorError> {
    let unit_ids = units_for_matrix_key(plan, &emit.matrix_key)?;
    let mut ordered_results = Vec::with_capacity(unit_ids.len());
    let mut observation_sidecars = Vec::with_capacity(unit_ids.len());
    for unit_id in &unit_ids {
        let entry = manifest
            .tasks
            .get(unit_id)
            .ok_or_else(|| internal(&format!("task_without_manifest_entry:{unit_id}")))?;
        let task_digest = obligation_digest(plan, unit_id)?;
        let report_id = task_report_id_for_task(run_key, &emit.matrix_key, task_digest)
            .map_err(internal_contract)?;
        let staged = read_staged_report(tasks_dir, &report_id)?;
        let sidecar = read_staged_sidecar(tasks_dir, &report_id)?;
        let result = unit_result_for_report(
            unit_id,
            entry,
            &report_id,
            staged.as_deref(),
            emit.job_status,
            finished_secs,
        )?;
        observation_sidecars.push(sidecar);
        ordered_results.push(result);
    }
    Ok((ordered_results, observation_sidecars))
}

/// Parse the on-disk task execution manifest: the checked-in document
/// carries a generated-marker header line (see `marked_json`) which is
/// stripped before strict parsing; a bare canonical document parses as-is.
fn parse_manifest_document(text: &str) -> Result<TaskExecutionManifestV1, OrchestratorError> {
    let body = match text.split_once('\n') {
        Some((first, rest)) if first.starts_with(velnor_actions_contract::MARKER_PREFIX) => {
            rest.strip_suffix('\n').unwrap_or(rest)
        }
        _ => text,
    };
    if body.bytes().any(|byte| matches!(byte, b'\n' | b'\r')) {
        return Err(internal("noncanonical_task_execution_manifest"));
    }
    let value = parse_strict_json(body).map_err(|_| internal("unparsable_manifest"))?;
    let manifest: TaskExecutionManifestV1 =
        serde_json::from_value(value).map_err(|_| internal("unparsable_manifest"))?;
    manifest.validate().map_err(internal_contract)?;
    Ok(manifest)
}

fn units_for_matrix_key(plan: &Plan, matrix_key: &str) -> Result<Vec<String>, OrchestratorError> {
    let mut units = std::collections::BTreeSet::new();
    for entry in &plan.matrix.include {
        if entry.matrix_key != matrix_key {
            continue;
        }
        for tasks in entry.execute_task_ids.tasks.values() {
            match tasks {
                velnor_actions_contract::ExecuteTaskRef::Single(id) => {
                    units.insert(id.clone());
                }
                velnor_actions_contract::ExecuteTaskRef::Shards(ids) => {
                    units.extend(ids.iter().cloned());
                }
            }
        }
    }
    if units.is_empty() {
        return Err(internal("matrix_key_not_in_plan"));
    }
    Ok(units.into_iter().collect())
}

fn obligation_digest<'a>(plan: &'a Plan, unit_id: &str) -> Result<&'a str, OrchestratorError> {
    plan.obligations
        .iter()
        .find(|obligation| obligation.task_id == unit_id)
        .map(|obligation| obligation.task_digest.as_str())
        .ok_or_else(|| internal(&format!("task_without_obligation:{unit_id}")))
}

fn split_job_key(job_key: &str) -> Result<(&str, ExecutionBranch), OrchestratorError> {
    if let Some(group) =
        job_key.strip_suffix(velnor_actions_contract::workflow::crate_job::GROUP_READER_SUFFIX)
    {
        if group.is_empty() {
            return Err(internal("job_key_without_group"));
        }
        return Ok((group, ExecutionBranch::Reader));
    }
    if let Some(group) =
        job_key.strip_suffix(velnor_actions_contract::workflow::crate_job::GROUP_WRITER_SUFFIX)
    {
        if group.is_empty() {
            return Err(internal("job_key_without_group"));
        }
        return Ok((group, ExecutionBranch::Writer));
    }
    Err(internal("job_key_without_branch_suffix"))
}

fn read_staged_report(
    tasks_dir: &Path,
    report_id: &str,
) -> Result<Option<String>, OrchestratorError> {
    let path = tasks_dir.join(format!("{report_id}.json"));
    match crate::retrieve_reports::read_staged_text(&path, MAX_REPORT_BYTES) {
        Ok(text) => Ok(Some(text)),
        Err("missing") => Ok(None),
        Err(kind) => Err(internal(&format!("{kind}_task_report:{report_id}"))),
    }
}

fn read_staged_sidecar(
    tasks_dir: &Path,
    report_id: &str,
) -> Result<Option<Vec<u8>>, OrchestratorError> {
    let path = tasks_dir.join(format!(
        "{report_id}{}",
        crate::task_report::UNIT_OBSERVATION_SUFFIX
    ));
    match crate::retrieve_reports::read_staged_text(&path, MAX_SIDECAR_BYTES) {
        Ok(text) => Ok(Some(text.into_bytes())),
        Err("missing") => Ok(None),
        Err(kind) => Err(internal(&format!("{kind}_observation:{report_id}"))),
    }
}

fn unit_result_for_report(
    unit_id: &str,
    entry: &TaskExecutionManifestEntryV1,
    report_id: &str,
    staged: Option<&str>,
    job_status: JobStatus,
    finished_secs: u64,
) -> Result<UnitResultV1, OrchestratorError> {
    let recipe_id =
        UnitResultV1::recipe_digest_from_manifest_entry(entry).map_err(internal_contract)?;
    let argv_digest =
        UnitResultV1::argv_digest_from_manifest_entry(entry).map_err(internal_contract)?;
    let effective_input_digest = UnitResultV1::effective_input_digest_from_manifest_entry(entry)
        .map_err(internal_contract)?;
    let task_identity = unit_id.to_owned();
    let Some(text) = staged else {
        // On success every selected unit must stage a report; on failure or
        // cancellation the missing unit is honestly recorded as
        // `NeedsPrepare` (evidence absent) rather than fabricated.
        if job_status == JobStatus::Success {
            return Err(internal(&format!("missing_task_report:{report_id}")));
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
        result.validate().map_err(internal_contract)?;
        return Ok(result);
    };
    let value = parse_strict_json(text).map_err(|_| internal("unparsable_task_report"))?;
    let report: TaskReport =
        serde_json::from_value(value).map_err(|_| internal("unparsable_task_report"))?;
    report.validate().map_err(internal_contract)?;
    let (outcome, exit_code) = outcome_for_report(&report, job_status, report_id)?;
    let duration_secs = report.duration_ms.unwrap_or(0) / 1_000;
    let (started_at, finished_at) = window_for_duration(finished_secs, duration_secs);
    let report_canonical =
        velnor_actions_contract::canonical_json_bytes(&report).map_err(internal_contract)?;
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
    result.validate().map_err(internal_contract)?;
    Ok(result)
}

fn outcome_for_report(
    report: &TaskReport,
    job_status: JobStatus,
    report_id: &str,
) -> Result<(UnitOutcome, Option<i32>), OrchestratorError> {
    let mapped = match (report.status, report.exit_code) {
        (TaskStatus::Executed, 0) => (UnitOutcome::Executed, Some(0)),
        (TaskStatus::Executed | TaskStatus::Failed, code) => (UnitOutcome::Failed, Some(code)),
        (TaskStatus::Cancelled, _) => (UnitOutcome::Cancelled, None),
        (TaskStatus::Reused, _) => (UnitOutcome::CacheHit, None),
        (TaskStatus::NotSelected | TaskStatus::EmptyPartition, _) => {
            (UnitOutcome::NeedsPrepare, None)
        }
    };
    // A successful job cannot carry failed or cancelled units: that evidence
    // contradicts the job conclusion, so emit refuses loudly instead of
    // writing a ledger the merge checker must reject.
    if job_status == JobStatus::Success
        && matches!(mapped.0, UnitOutcome::Failed | UnitOutcome::Cancelled)
    {
        return Err(internal(&format!("failed_unit_on_success:{report_id}")));
    }
    Ok(mapped)
}

fn window_for_duration(finished_secs: u64, duration_secs: u64) -> (String, String) {
    let started_secs = finished_secs.saturating_sub(duration_secs);
    (utc_rfc3339(started_secs), utc_rfc3339(finished_secs))
}

fn clock_secs() -> Result<u64, OrchestratorError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| internal("missing_clock"))
}

fn utc_rfc3339(secs: u64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(i64::try_from(days).unwrap_or(0));
    let hour = rest / 3_600;
    let minute = rest % 3_600 / 60;
    let second = rest % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
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

// gc1-t54 covers group-ledger emit: the success path writes a validated
// ledger whose every digest recomputes from the staged inputs, a missing
// report on success is a loud error (red first), a missing report on
// failure records NeedsPrepare, and a failed unit on success is refused.
#[cfg(test)]
mod gc1_t54 {
    use velnor_actions_contract::workflow::{UnitLedgerV1, UnitOutcome, UnitResultV1};

    use super::super::gc1_t54_support::{self, RUN_KEY};
    use super::{JobStatus, emit_group_ledger_to};

    fn fail(problem: &str) -> crate::OrchestratorError {
        crate::internal::internal(problem)
    }

    #[test]
    fn success_emits_validated_ledger() -> Result<(), crate::OrchestratorError> {
        let fixture = gc1_t54_support::build(JobStatus::Success)?;
        let report_file = format!("{}.json", fixture.report_id.as_str());
        let out = emit_group_ledger_to(
            &fixture.anchor,
            &fixture.manifest_path,
            &fixture.run_dir,
            RUN_KEY,
            &fixture.emit_env,
        )?;
        assert_eq!(
            out,
            fixture
                .run_dir
                .join(&fixture.emit_env.matrix_key)
                .join(super::GROUP_LEDGER_FILENAME)
        );
        let text = std::fs::read_to_string(&out).map_err(|err| fail(&err.to_string()))?;
        let ledger = UnitLedgerV1::parse_json(&text).map_err(|_| fail("ledger_parse"))?;
        ledger.validate().map_err(|_| fail("ledger_invalid"))?;
        assert_eq!(ledger.selected_unit_ids, vec![fixture.task_id.clone()]);
        assert_eq!(ledger.ordered_results.len(), 1);
        let result = &ledger.ordered_results[0];
        assert_eq!(result.unit_id, fixture.task_id);
        assert_eq!(result.outcome, UnitOutcome::Executed);
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(ledger.job.repository_id, 0, "tokenless numerics stay zero");
        assert_eq!(ledger.job.job_id, 0, "tokenless numerics stay zero");
        assert_eq!(ledger.job.repository, "tailrocks/velnor-new");
        assert_eq!(ledger.job.run_id, 4242);
        assert_eq!(ledger.job.run_attempt, 1);
        assert_eq!(ledger.job.source_sha, gc1_t54_support::HEAD_SHA);
        assert!(
            result.started_at < result.finished_at,
            "finished=now started=now-duration"
        );
        let plan_bytes = std::fs::read(fixture.run_dir.join("plan.json"))
            .map_err(|err| fail(&err.to_string()))?;
        let plan_digest = UnitLedgerV1::plan_digest_from_canonical_bytes(&plan_bytes);
        assert_eq!(ledger.job.plan_digest.as_str(), plan_digest.as_str());
        assert_eq!(
            ledger.group_digest.as_str(),
            UnitLedgerV1::group_digest_for_group("t54-group", &plan_digest)
                .map_err(|_| fail("group"))?
                .as_str(),
        );
        let report_bytes = std::fs::read(
            fixture
                .run_dir
                .join(&fixture.emit_env.matrix_key)
                .join("tasks")
                .join(report_file.as_str()),
        )
        .map_err(|err| fail(&err.to_string()))?;
        assert_eq!(
            result.report_digest.as_str(),
            UnitResultV1::report_digest_from_canonical_bytes(&report_bytes).as_str(),
        );
        assert_eq!(
            ledger.observation_digest.as_str(),
            UnitLedgerV1::observation_digest_for_sidecars(&[None])
                .map_err(|_| fail("observation"))?
                .as_str(),
        );
        Ok(())
    }

    #[test]
    fn missing_report_on_success_is_loud() -> Result<(), crate::OrchestratorError> {
        let fixture = gc1_t54_support::build(JobStatus::Success)?;
        let report_file = format!("{}.json", fixture.report_id.as_str());
        std::fs::remove_file(
            fixture
                .run_dir
                .join(&fixture.emit_env.matrix_key)
                .join("tasks")
                .join(report_file.as_str()),
        )
        .map_err(|err| fail(&err.to_string()))?;
        match emit_group_ledger_to(
            &fixture.anchor,
            &fixture.manifest_path,
            &fixture.run_dir,
            RUN_KEY,
            &fixture.emit_env,
        ) {
            Ok(_) => Err(fail("missing report on success must fail loudly")),
            Err(err) => {
                assert!(
                    err.to_string().contains("missing_task_report"),
                    "unexpected error: {err}"
                );
                Ok(())
            }
        }
    }

    #[test]
    fn missing_report_on_failure_defers() -> Result<(), crate::OrchestratorError> {
        let fixture = gc1_t54_support::build(JobStatus::Failure)?;
        let report_file = format!("{}.json", fixture.report_id.as_str());
        std::fs::remove_file(
            fixture
                .run_dir
                .join(&fixture.emit_env.matrix_key)
                .join("tasks")
                .join(report_file.as_str()),
        )
        .map_err(|err| fail(&err.to_string()))?;
        let out = emit_group_ledger_to(
            &fixture.anchor,
            &fixture.manifest_path,
            &fixture.run_dir,
            RUN_KEY,
            &fixture.emit_env,
        )?;
        let text = std::fs::read_to_string(&out).map_err(|err| fail(&err.to_string()))?;
        let ledger = UnitLedgerV1::parse_json(&text).map_err(|_| fail("ledger_parse"))?;
        assert_eq!(ledger.ordered_results.len(), 1);
        assert_eq!(ledger.ordered_results[0].outcome, UnitOutcome::NeedsPrepare);
        assert_eq!(ledger.ordered_results[0].exit_code, None);
        assert_eq!(
            ledger.ordered_results[0].report_digest.as_str(),
            UnitResultV1::missing_report_digest().as_str(),
        );
        Ok(())
    }
}

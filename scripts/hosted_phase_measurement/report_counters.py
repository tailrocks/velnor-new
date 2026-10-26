"""Summarize counters actually present in exact joined step logs."""

from __future__ import annotations

import re
from typing import Any

from .report_events import resolve_effective_job

_SOURCE_SHA = re.compile(r"^[0-9a-f]{40}$")


CACHE_PHASES = {"cargo_source_restore", "cargo_source_save", "mbx_object_restore",
                "mbx_object_post_upload", "mbx_bundle_restore", "mbx_bundle_import",
                "mbx_bundle_export", "mbx_bundle_save"}
RESTORE_PHASES = {"cargo_source_restore", "mbx_object_restore", "mbx_bundle_restore"}
SAVE_PHASES = {"cargo_source_save", "mbx_bundle_save", "mbx_object_post_upload"}


def _transfer_totals(event_steps: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], dict[str, dict[str, Any]]]:
    totals: dict[str, dict[str, Any]] = {}
    observations = []
    for step in event_steps:
        if step["phase"] not in RESTORE_PHASES | SAVE_PHASES or step["conclusion"] == "skipped":
            continue
        row, transferred, direction = _transfer_row(step)
        observations.append(row)
        if transferred is not None:
            _add_transfer_total(totals, step, direction, transferred)
    return observations, totals


def _transfer_row(step: dict[str, Any]) -> tuple[dict[str, Any], int | None, str]:
    evidence = step.get("log_evidence") or {}
    direction = "restore" if step["phase"] in RESTORE_PHASES else "save"
    received = evidence.get("cache_bytes_received") if direction == "restore" else None
    uploaded = evidence.get("cache_bytes_uploaded") if direction == "save" else None
    transferred = received if direction == "restore" else uploaded
    unknown = "No cumulative sent/received byte counter was present in this step log."
    duration_unknown = evidence.get("cache_transfer_duration_unknown_reason", "No distinct transfer start/end timestamps are present in the retained step/API record.")
    row = {"job_id": step["job_id"], "job_name": step["job_name"], "step_name": step["step_name"],
           "phase": step["phase"], "direction": direction,
           "enclosing_api_step_span_ms": step["duration_ms_from_api_timestamps"],
           "log_reported_received_bytes": received, "log_reported_uploaded_bytes": uploaded,
           "authoritative_bytes": transferred, "byte_counter_unknown_reason": None if transferred is not None else unknown,
           "transfer_duration_ms": None, "transfer_duration_unknown_reason": duration_unknown,
           "log_sha256": evidence.get("log_sha256")}
    return row, transferred, direction


def _add_transfer_total(totals: dict[str, dict[str, Any]], step: dict[str, Any], direction: str, amount: int) -> None:
    phase = step["phase"]
    total = totals.setdefault(phase, {"direction": direction, "counter_steps": 0,
                                      "total_bytes": 0, "enclosing_api_step_span_ms_sum": 0})
    total["counter_steps"] += 1
    total["total_bytes"] += amount
    total["enclosing_api_step_span_ms_sum"] += step["duration_ms_from_api_timestamps"] or 0


def _evidence_rows(event_steps: list[dict[str, Any]], key: str) -> list[Any]:
    return [row for step in event_steps for row in (step.get("log_evidence") or {}).get(key, [])]


def build_actual_counters(event_steps: list[dict[str, Any]], timeline: dict[str, Any]) -> dict[str, Any]:
    cache_steps = [step for step in event_steps if step["phase"] in CACHE_PHASES]
    transfers, totals = _transfer_totals(event_steps)
    mbx_rows = _evidence_rows(event_steps, "mbx_object_cache_summaries")
    actions = sorted(set(_evidence_rows(event_steps, "pinned_action_refs")))
    return {
        "pinned_action_refs_observed_in_logs": actions, "cache_steps": cache_steps,
        "cache_transfer_observations": transfers, "cache_transfer_totals_by_phase": totals,
        "mise_download_progress": _evidence_rows(event_steps, "mise_download_progress"),
        "cargo_download_package_lines": [line for step in event_steps if step["phase"] == "cargo_fetch" for line in (step.get("log_evidence") or {}).get("cargo_download_package_lines", [])],
        "mbx_object_cache_summaries": mbx_rows,
        "mbx_object_cache_summary_totals": {"observation_count": len(mbx_rows),
            "remote_downloaded_bytes": sum(row["remote_downloaded_bytes"] for row in mbx_rows),
            "remote_uploaded_bytes": sum(row["remote_uploaded_bytes"] for row in mbx_rows),
            "lookup_totals": {label: sum(row["counts"].get(label, 0) for row in mbx_rows)
                              for label in ("hits", "misses", "not_looked_up", "bypassed")},
            "scope": "sum of actual MBX summary counters only; zero is not compiler freshness or full cache-transfer telemetry"},
        **_unknown_actual_counters(timeline),
    }


def build_lifecycle_aggregates(*, repository: str, run_id: int, head_sha: str,
                               jobs: list[dict[str, Any]],
                               effective_jobs: dict[tuple[str, int], dict[str, Any]],
                               collected: dict[str, Any]) -> dict[str, Any]:
    """Aggregate complete lifecycle work from verified owned observations.

    Every accepted observation is resolved through the authenticated
    effective-job map by its actual producer job/attempt, so a forged or
    stale attempt fails instead of joining. Each real job, attempt, and
    bound task report is counted exactly once: a retained successful job
    contributes its original work once, and a rerun contributes its new
    work in addition to earlier work because it carries a distinct actual
    attempt. Executed lifecycle work covers every supplied authenticated
    job record (bootstrap, helpers, preparation, replay, failed attempts)
    with no role or conclusion filter, while effective Required results
    cover exactly the retained-or-rerun jobs accepted for the current
    verdict. Anything without a bound observation stays unknown, never
    zero. Origin bytes, cache bytes, and historical MBX summaries are kept
    in separate channels and never summed together.
    """
    if not isinstance(repository, str) or "/" not in repository or not repository.strip():
        raise ValueError("lifecycle aggregates: repository is not an owner/repo path")
    if type(run_id) is not int or run_id <= 0:
        raise ValueError("lifecycle aggregates: run_id is not a positive integer")
    if not isinstance(head_sha, str) or not _SOURCE_SHA.match(head_sha):
        raise ValueError("lifecycle aggregates: head_sha is not the authenticated run source")
    if not isinstance(collected, dict):
        raise ValueError("lifecycle aggregates: collected observations are missing")
    accepted = collected.get("accepted")
    missing_reports = collected.get("missing_task_report_observations")
    if not isinstance(accepted, dict) or not isinstance(missing_reports, list):
        raise ValueError("lifecycle aggregates: collected observations lack accepted/missing bindings")
    joined: list[dict[str, Any]] = []
    seen_join_keys: set[tuple[str, int, int, int, str, str]] = set()
    for task_report_id in sorted(accepted):
        record = accepted[task_report_id]
        if not isinstance(record, dict):
            raise ValueError(f"lifecycle join: observation {task_report_id} is not a record")
        job = resolve_effective_job(effective_jobs, producer_job=record["producer_job"],
                                    producer_attempt=record["producer_attempt"])
        if job["id"] != record.get("effective_job_id"):
            raise ValueError(f"lifecycle join: observation {task_report_id} bound job id "
                             f"{record.get('effective_job_id')} differs from effective record {job['id']}")
        key = (repository, run_id, job["id"], record["producer_attempt"],
               record["observation_session_id"], task_report_id)
        if key in seen_join_keys:
            raise ValueError(f"lifecycle join: duplicate observation for {key}")
        seen_join_keys.add(key)
        joined.append({"task_report_id": task_report_id,
                       "producer_job": record["producer_job"],
                       "producer_attempt": record["producer_attempt"],
                       "effective_job_id": job["id"],
                       "effective_job_conclusion": job.get("conclusion"),
                       "observation_session_id": record["observation_session_id"]})
    executed: list[dict[str, Any]] = []
    seen_job_ids: dict[int, tuple[Any, Any]] = {}
    for job in jobs:
        job_id = job.get("id")
        if type(job_id) is not int or job_id <= 0:
            raise ValueError("lifecycle aggregates: a supplied job lacks a numeric job id")
        identity = (job.get("name"), job.get("run_attempt"))
        if job_id in seen_job_ids:
            raise ValueError(f"lifecycle aggregates: duplicate supplied record for job id {job_id}")
        seen_job_ids[job_id] = identity
        executed.append({"job_id": job_id, "job_name": job.get("name"),
                         "run_attempt": job.get("run_attempt"),
                         "status": job.get("status"), "conclusion": job.get("conclusion")})
    executed.sort(key=lambda row: row["job_id"])
    by_conclusion: dict[str, int] = {}
    for row in executed:
        by_conclusion[str(row["conclusion"])] = by_conclusion.get(str(row["conclusion"]), 0) + 1
    covered_ids = {row["effective_job_id"] for row in joined}
    pair_tasks: dict[tuple[int, int], list[str]] = {}
    for row in joined:
        pair_tasks.setdefault((row["effective_job_id"], row["producer_attempt"]), []).append(row["task_report_id"])
    covered = [{"job_id": job_id, "job_name": next(row["producer_job"] for row in joined
                                                   if row["effective_job_id"] == job_id),
                "actual_attempt": attempt,
                "bound_task_reports": sorted(pair_tasks[(job_id, attempt)])}
               for job_id, attempt in sorted(pair_tasks)]
    uncovered = [row for row in executed if row["job_id"] not in covered_ids]
    return {
        "status": "verified",
        "repository": repository,
        "run_id": run_id,
        "head_sha": head_sha,
        "dedup_key": ["repository", "run_id", "actual job id", "actual attempt",
                      "observation session", "task report id"],
        "dedup_scope": "each real job, attempt, and bound task report is counted exactly once; "
                       "a retained job contributes its original work once and a rerun contributes "
                       "its new work in addition to earlier work",
        "join": {"status": "verified",
                 "method": "each owned observation resolved through the authenticated effective-job "
                           "map by actual producer job/attempt; forged or stale attempts fail instead of joining",
                 "observation_count": len(joined),
                 "joined": joined},
        "executed_lifecycle": {
            "job_count": len(executed),
            "jobs": executed,
            "by_conclusion": by_conclusion,
            "scope": "every supplied authenticated job record counted once; includes bootstrap, helpers, "
                     "preparation, replay, and failed or cancelled attempts with no role or conclusion filter",
        },
        "effective_required": {
            "covered_job_attempt_count": len(covered),
            "covered_job_attempts": covered,
            "bound_task_report_count": len(joined),
            "scope": "exactly the retained-or-rerun jobs accepted for the current verdict",
        },
        "missing_coverage": {
            "jobs_without_bound_observation": [
                {**row, "disposition": "unknown",
                 "unknown_reason": "No bound observation member covers this job; its work stays unknown, never zero."}
                for row in uncovered],
            "task_reports_without_observation": missing_reports,
            "disposition": "unknown",
            "unknown_reason": "Jobs and task reports without a bound observation stay unknown; "
                              "they are never counted as zero work.",
        },
        "process_counts": {
            "compiler_executions": None,
            "compiler_executions_unknown_reason": "Owned bindings carry identity, not per-process compiler "
                                                 "events; unobserved process work stays unknown, never zero.",
            "native_process_executions": None,
            "native_process_executions_unknown_reason": "Absolute native calls and unobserved build-script "
                                                       "network access are not covered by owned bindings; "
                                                       "they stay unknown, never zero.",
        },
        "byte_channels": {
            "origin_acquisition_bytes": None,
            "origin_acquisition_bytes_unknown_reason": "Owned observation bindings do not join to origin "
                                                      "receipts in this collector; origin bytes stay unknown here.",
            "cache_transfer_bytes": None,
            "cache_transfer_bytes_unknown_reason": "Authoritative only in "
                                                   "actual_counters.cache_transfer_totals_by_phase; the lifecycle "
                                                   "view never re-sums them.",
            "mbx_object_cache_summaries": None,
            "mbx_object_cache_summaries_unknown_reason": "Corroborating observations only in "
                                                        "actual_counters.mbx_object_cache_summary_totals; never "
                                                        "promoted into compiler or cache-transfer counts.",
            "separation_rule": "Origin bytes, cache bytes, and historical MBX summaries are kept separate "
                               "and never summed together.",
        },
    }


def _unknown_actual_counters(timeline: dict[str, Any]) -> dict[str, Any]:
    return {
        "tool_download_duration_ms": None,
        "tool_download_duration_unknown_reason": "Mise log lines contain progress snapshots, not a unique completed transfer interval; enclosing Prepare pinned tools API step span is reported by phase.",
        "tool_download_bytes": None,
        "tool_download_bytes_unknown_reason": "Pinned Mise progress output has human-readable progress, not a stable authoritative byte counter.",
        "cargo_download_bytes": None,
        "cargo_download_bytes_unknown_reason": "Cargo log output does not provide authoritative per-package transferred byte counts.",
        "link_duration_ms": None,
        "link_duration_unknown_reason": "No separate linker start/end event is present; Cargo build/test step spans combine compiler and link work.",
        "fresh_compiler_units": None,
        "fresh_compiler_units_unknown_reason": "The retained run has no complete compiler-unit receipt; Compiling lines and MBX lookup counters are not unit totals.",
        "lock_wait_ms": None,
        "lock_wait_unknown_reason": "Hosted APIs and retained logs contain no separate lock-wait measurement; task report zero sentinels are unavailable telemetry.",
        "runner_queue_duration_ms": None,
        "runner_queue_duration_unknown_reason": timeline["runner_queue_unknown_reason"],
        "native_cpu": None,
        "native_cpu_unknown_reason": "Local compiler observer receipts lack exact hosted run/job/attempt/source identity and are not joinable to this run.",
    }

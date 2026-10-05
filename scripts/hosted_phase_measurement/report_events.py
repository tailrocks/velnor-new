"""Build job and step event rows from exact API data and archive joins."""

from __future__ import annotations

from datetime import datetime
from typing import Any

from .measure_support import duration_ms, extract_log_evidence, parse_time, safe_log_key
from .step_phases import STEP_PHASES, normalize_step_name, phase_for_step


def collect_job_events(jobs: list[dict[str, Any]], log_entries: dict[str, bytes]) -> dict[str, Any]:
    job_rows, event_steps, missing = [], [], []
    phase_worker_ms: dict[str, int] = {}
    started: list[tuple[datetime, int]] = []
    completed: list[tuple[datetime, int]] = []
    for job in jobs:
        _append_job_times(job, started, completed)
        steps_out = _collect_job_steps(job, log_entries, event_steps, missing, phase_worker_ms)
        job_rows.append(_job_row(job, steps_out))
    return {"jobs": job_rows, "steps": event_steps, "missing_logs": missing,
            "phase_worker_ms": phase_worker_ms, "started": started, "completed": completed}


def _append_job_times(job: dict[str, Any], started: list[tuple[datetime, int]], completed: list[tuple[datetime, int]]) -> None:
    start = parse_time(job.get("started_at"), f"job {job['id']}.started_at")
    end = parse_time(job.get("completed_at"), f"job {job['id']}.completed_at")
    if start:
        started.append((start, job["id"]))
    if end:
        completed.append((end, job["id"]))


def _collect_job_steps(job: dict[str, Any], entries: dict[str, bytes], all_steps: list[dict[str, Any]], missing: list[dict[str, Any]], phase_ms: dict[str, int]) -> list[dict[str, Any]]:
    rows = []
    for step in job.get("steps", []):
        row = _step_row(job, step, entries)
        expected_log = row.pop("expected_log_file")
        rows.append(row)
        all_steps.append(row)
        span = row["duration_ms_from_api_timestamps"]
        if span is not None:
            phase_ms[row["phase"]] = phase_ms.get(row["phase"], 0) + span
        if row["log_evidence"] is None and step.get("status") == "completed" and step.get("conclusion") != "skipped":
            missing.append({"job_id": job["id"], "step_number": step.get("number"), "step_name": step.get("name"), "expected_log_file": expected_log})
    return rows


def _step_row(job: dict[str, Any], step: dict[str, Any], entries: dict[str, bytes]) -> dict[str, Any]:
    phase = phase_for_step(step.get("name", ""))
    log_key = safe_log_key(job["name"], step)
    raw = entries.get(log_key)
    span = duration_ms(step.get("started_at"), step.get("completed_at"), f"job {job['id']} step {step.get('number')}")
    return {"job_id": job["id"], "job_name": job["name"], "step_number": step.get("number"),
            "step_name": step.get("name"), "phase": phase, "started_at": step.get("started_at"),
            "completed_at": step.get("completed_at"), "duration_ms_from_api_timestamps": span,
            "api_timestamp_precision_ms": 1000, "status": step.get("status"),
            "conclusion": step.get("conclusion"),
            "log_evidence": extract_log_evidence(raw, phase, log_key) if raw is not None else None,
            "expected_log_file": log_key}


def _job_row(job: dict[str, Any], steps: list[dict[str, Any]]) -> dict[str, Any]:
    job_id = job["id"]
    return {"job_id": job_id, "job_name": job["name"], "run_id": job["run_id"],
            "run_attempt": job["run_attempt"], "head_sha": job["head_sha"],
            "runner_labels": job.get("labels", []), "runner_name": job.get("runner_name"),
            "runner_id": job.get("runner_id"), "status": job.get("status"),
            "conclusion": job.get("conclusion"), "created_at": job.get("created_at"),
            "started_at": job.get("started_at"), "completed_at": job.get("completed_at"),
            "created_to_started_ms_unclassified": duration_ms(job.get("created_at"), job.get("started_at"), f"job {job_id} prestart"),
            "queue_duration_ms": None,
            "queue_unknown_reason": "GitHub jobs response has no queue timestamp; created-to-started can include dependency wait and scheduler/runner startup.",
            "duration_ms_from_api_timestamps": duration_ms(job.get("started_at"), job.get("completed_at"), f"job {job_id} wall"),
            "steps": steps}


def workflow_timeline(run: dict[str, Any], jobs: list[dict[str, Any]], events: dict[str, Any], dag_path: dict[str, Any]) -> dict[str, Any]:
    started, completed = events["started"], events["completed"]
    first = min(started) if started else None
    last = max(completed) if completed else None
    start_at = first[0].isoformat().replace("+00:00", "Z") if first else None
    end_at = last[0].isoformat().replace("+00:00", "Z") if last else None
    dag = dag_path.get("duration_ms")
    run_first_start = min((job.get("started_at") for job in jobs if job.get("started_at")), default=None)
    return {"first_job_started_at": start_at, "first_job_id": first[1] if first else None,
            "last_job_completed_at": end_at, "last_job_id": last[1] if last else None,
            "observed_job_span_envelope_ms": int((last[0] - first[0]).total_seconds() * 1000) if first and last else None,
            "observed_job_span_method": "latest job completion minus earliest job start; envelope only, not a DAG critical path",
            "workflow_critical_path_ms": dag,
            "workflow_critical_path_status": dag_path.get("status"),
            "workflow_critical_path_unknown_reason": dag_path.get("reason"),
            "workflow_critical_path_evidence": dag_path if dag_path.get("status") == "verified" else None,
            "run_created_at": run.get("created_at"),
            "run_created_to_first_job_start_ms_unclassified": duration_ms(run.get("created_at"), run_first_start, "run prestart") if started else None,
            "runner_queue_duration_ms": None,
            "runner_queue_unknown_reason": "The retained workflow-run and jobs APIs expose no queue-start/end pair or dependency-wait split."}


def phase_worker_time(events: dict[str, Any]) -> dict[str, Any]:
    steps = events["steps"]
    return {name: {"cumulative_job_step_ms": ms,
                   "step_count": sum(1 for row in steps if row["phase"] == name and row["duration_ms_from_api_timestamps"] is not None),
                   "interpretation": "sum of API step spans across workers; parallel spans can overlap and this is not wall-clock critical path"}
            for name, ms in sorted(events["phase_worker_ms"].items())}


def phase_map(events: dict[str, Any]) -> dict[str, Any]:
    counts: dict[tuple[str, str], int] = {}
    for step in events["steps"]:
        key = (step["step_name"], step["phase"])
        counts[key] = counts.get(key, 0) + 1
    observed = [{"api_step_name": name, "normalized_step_name": normalize_step_name(name),
                 "phase": phase, "api_step_count": count}
                for (name, phase), count in sorted(counts.items())]
    return {"normalization": "case-fold, replace punctuation with spaces, collapse tokens, then exact whole-name lookup",
            "configured_normalized_name_map": STEP_PHASES, "observed_step_name_map": observed}

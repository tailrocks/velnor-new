"""Assemble source-bound phase reports from retained API and log evidence."""

from __future__ import annotations

import zipfile
from argparse import Namespace
from pathlib import Path
from typing import Any

from .hosted_dag import compute_workflow_dag_path
from .measure_support import (SCHEMA, file_receipt, inspect_task_reports, load_json,
                              read_step_logs, validate_actual_counter_unknowns,
                              validate_artifact, validate_identity, workflow_receipt)
from .report_counters import build_actual_counters
from .report_events import collect_job_events, phase_map, phase_worker_time, workflow_timeline


def _workflow_path(workflow: dict[str, Any], run: dict[str, Any], attempt: int,
                   jobs: list[dict[str, Any]], events: dict[str, Any]) -> dict[str, Any]:
    graph = workflow.get("job_graph", {})
    path = compute_workflow_dag_path(graph, jobs, events["steps"], events["missing_logs"])
    if path.get("status") != "verified":
        return path
    return {**path, "run_id": run["id"], "run_attempt": attempt, "head_sha": run["head_sha"],
            "workflow_path": workflow.get("path"), "workflow_sha256": workflow.get("sha256"),
            "workflow_git_blob_sha": workflow.get("git_blob_sha"),
            "workflow_job_graph_sha256": graph.get("sha256")}


def _archive_receipt(args: Namespace, run: dict[str, Any], events: dict[str, Any], counts: dict[str, int]) -> dict[str, Any]:
    joined = sum(1 for row in events["steps"] if row["log_evidence"] is not None)
    return {**file_receipt(args.logs_zip, "actions_run_logs_zip"), **counts,
            "joined_api_step_log_count": joined,
            "source_endpoint": f"GET /repos/{args.repository}/actions/runs/{run['id']}/logs",
            "entry_count_method": "ZIP count includes all archive entries; candidates are immediate job-folder .txt files, including summary files; joins require exact API job and step archive paths."}


def _source_receipts(args: Namespace, run: dict[str, Any], jobs_path: Path, workflow: dict[str, Any],
                     artifact: dict[str, Any] | None, events: dict[str, Any], archive_counts: dict[str, int]) -> dict[str, Any]:
    return {"run_metadata": file_receipt(args.run_json, "actions_run_api_projection"),
            "jobs_api": file_receipt(jobs_path, "actions_jobs_api_page"),
            "logs_archive": _archive_receipt(args, run, events, archive_counts),
            "workflow": workflow, "final_report_artifact": artifact}


def build_report(args: Namespace) -> dict[str, Any]:
    run = load_json(args.run_json)
    jobs_doc = load_json(args.jobs_json)
    jobs = validate_identity(run, jobs_doc, args.repository)
    attempt = run["run_attempt"]
    artifact_doc, artifact_receipt = validate_artifact(args.artifact_json, args.artifact_id, args.final_artifact, run, args.repository)
    workflow = workflow_receipt(args.workflow_json, args.workflow_file, run, args.repository)
    with zipfile.ZipFile(args.logs_zip) as archive:
        log_entries, archive_counts = read_step_logs(archive)
    events = collect_job_events(jobs, log_entries)
    dag_path = _workflow_path(workflow, run, attempt, jobs, events)
    timeline = workflow_timeline(run, jobs, events, dag_path)
    counters = build_actual_counters(events["steps"], timeline)
    validate_actual_counter_unknowns(counters)
    return {
        "schema": SCHEMA,
        "measurement_policy": {"durations": "GitHub Actions run/job/step API start and completion timestamps only",
            "bytes": "log-reported cumulative received/uploaded counters only; artifact/cache stored size is never presented as network transfer",
            "unknowns": "null plus reason when an API/log counter cannot isolate a requested phase",
            "compiler": "Cargo Compiling text and MBX not-looked-up/miss counts do not estimate total fresh compiler/link work"},
        "run_identity": {"repository": args.repository, "run_id": run["id"], "run_attempt": attempt,
            "event": run.get("event"), "head_sha": run["head_sha"], "workflow_id": run.get("workflow_id"),
            "workflow_path": run.get("path"), "status": run.get("status"), "conclusion": run.get("conclusion")},
        "source_receipts": _source_receipts(args, run, args.jobs_json, workflow, artifact_receipt, events, archive_counts),
        "timeline": timeline, "phase_worker_time": phase_worker_time(events), "phase_map": phase_map(events),
        "jobs": events["jobs"], "actual_counters": counters,
        "legacy_task_report_telemetry": inspect_task_reports(args.task_report_dir, run["id"], attempt, run["head_sha"], artifact_doc),
        "log_coverage": {"completed_steps_without_per_step_log": events["missing_logs"],
            "note": "Missing step logs leave only the API span; they are not backfilled from aggregate job logs."},
    }

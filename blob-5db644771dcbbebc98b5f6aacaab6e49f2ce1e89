"""Strict validation and parsing shared by the hosted measurement collector."""

from __future__ import annotations

import base64
import hashlib
import json
import re
import zipfile
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Any

from .hosted_dag import parse_workflow_job_graph
from .mbx_summary import parse_mbx_summary

SCHEMA = "velnor.hosted-phase-measurement.v4"
STAGE_TIMINGS = (
    "queue_ms", "prep_ms", "download_ms", "compiler_ms", "link_ms",
    "test_ms", "lock_wait_ms", "mbx_ms", "cache_ms", "runner_ms",
)
RECEIVED = re.compile(r"\bReceived\s+(\d+)\s+of\s+(\d+)(?:\s+\(([^)]*)\))?", re.I)
SENT = re.compile(r"\bSent\s+(\d+)\s+of\s+(\d+)(?:\s+\(([^)]*)\))?", re.I)
ACTION_RUN = re.compile(r"\bRun\s+([A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+)@([0-9a-f]{40})\b")
TIME_PREFIX = re.compile(r"^(?:\ufeff)?(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z)\s?(.*)$")
TOOL_PROGRESS = re.compile(
    r"(?P<tool>rust|mr-boxington)@(?P<version>[0-9.]+).*?"
    r"(?P<verb>Downloading|downloading)\s+(?P<asset>[^\s]+)"
    r"\s+(?P<elapsed>[\d.]+)s\s+(?P<current>[\d.]+)/(?P<total>[\d.]+)\s*(?P<unit>kB|MB|GB|B)\b",
    re.I,
)
CARGO_DOWNLOAD = re.compile(r"^\s*Downloading\s+(.+?)\s*$")
UNKNOWN_ACTUAL_COUNTER_REASON_FIELDS = {
    "tool_download_duration_ms": "tool_download_duration_unknown_reason",
    "tool_download_bytes": "tool_download_bytes_unknown_reason",
    "cargo_download_bytes": "cargo_download_bytes_unknown_reason",
    "link_duration_ms": "link_duration_unknown_reason",
    "fresh_compiler_units": "fresh_compiler_units_unknown_reason",
    "lock_wait_ms": "lock_wait_unknown_reason",
    "runner_queue_duration_ms": "runner_queue_duration_unknown_reason",
    "native_cpu": "native_cpu_unknown_reason",
}


def fail(message: str) -> None:
    raise ValueError(message)


def validate_actual_counter_unknowns(actual_counters: dict[str, Any]) -> None:
    """Reject numeric sentinels for counters whose hosted evidence is unavailable."""
    for counter, reason_field in UNKNOWN_ACTUAL_COUNTER_REASON_FIELDS.items():
        if counter not in actual_counters:
            fail(f"actual_counters.{counter} is missing")
        if actual_counters[counter] is not None:
            fail(f"actual_counters.{counter} must be null until measured")
        reason = actual_counters.get(reason_field)
        if not isinstance(reason, str) or not reason.strip():
            fail(f"actual_counters.{reason_field} must explain why the measurement is unknown")


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def file_receipt(path: Path, label: str) -> dict[str, Any]:
    payload = path.read_bytes()
    return {"label": label, "file_name": path.name, "size_bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}


def parse_time(value: Any, label: str) -> datetime | None:
    if value is None:
        return None
    if not isinstance(value, str):
        fail(f"{label}: timestamp is not a string")
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        fail(f"{label}: invalid timestamp {value!r}: {error}")
    if parsed.tzinfo is None:
        fail(f"{label}: timestamp has no timezone")
    return parsed.astimezone(timezone.utc)


def duration_ms(start: Any, end: Any, label: str) -> int | None:
    first = parse_time(start, f"{label}.started_at")
    last = parse_time(end, f"{label}.completed_at")
    if first is None or last is None:
        return None
    if last < first:
        fail(f"{label}: end precedes start")
    return int((last - first).total_seconds() * 1000)


def read_step_logs(archive: zipfile.ZipFile) -> tuple[dict[str, bytes], dict[str, int]]:
    """Return selected two-part text candidates and the raw ZIP entry count."""
    selected: dict[str, bytes] = {}
    entries = archive.infolist()
    candidates = 0
    for entry in entries:
        path = PurePosixPath(entry.filename)
        if len(path.parts) != 2 or not path.parts[1].endswith(".txt"):
            continue
        candidates += 1
        selected[entry.filename] = archive.read(entry)
    counts = {"zip_entry_count_total": len(entries), "selected_two_part_txt_candidate_count": candidates}
    return selected, counts


def safe_log_key(job_name: str, step: dict[str, Any]) -> str:
    folder = job_name.replace("/", "_")
    step_name = str(step["name"]).replace("/", "_")
    return f"{folder}/{step['number']}_{step_name}.txt"


def extract_log_evidence(raw: bytes, phase: str, log_name: str) -> dict[str, Any]:
    text = raw.decode("utf-8", errors="replace").replace("\r\n", "\n")
    received: list[dict[str, Any]] = []
    sent: list[dict[str, Any]] = []
    mbx_rows: list[dict[str, Any]] = []
    action_refs: list[str] = []
    tool_rows: list[dict[str, Any]] = []
    cargo_rows: list[str] = []
    for raw_line in text.splitlines():
        match = TIME_PREFIX.match(raw_line)
        stamp, body = (match.group(1), match.group(2)) if match else (None, raw_line)
        clean = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", body)
        counter = RECEIVED.search(clean)
        if counter:
            received.append({"timestamp": stamp, "received_bytes": int(counter.group(1)), "reported_total_bytes": int(counter.group(2)), "reported_rate": counter.group(3)})
        upload = SENT.search(clean)
        if upload:
            sent.append({"timestamp": stamp, "uploaded_bytes": int(upload.group(1)), "reported_total_bytes": int(upload.group(2)), "reported_rate": upload.group(3)})
        action = ACTION_RUN.search(clean)
        if action:
            action_refs.append(f"{action.group(1)}@{action.group(2)}")
        progress = TOOL_PROGRESS.search(clean)
        if progress:
            tool_rows.append({"timestamp": stamp, "tool": progress.group("tool").lower(), "version": progress.group("version"), "asset": progress.group("asset"), "reported_elapsed_seconds": float(progress.group("elapsed")), "reported_progress": f"{progress.group('current')}/{progress.group('total')} {progress.group('unit')}", "completion_proven": False})
        if phase == "cargo_fetch":
            download = CARGO_DOWNLOAD.match(clean)
            if download:
                cargo_rows.append(download.group(1))
        mbx = parse_mbx_summary(clean)
        if mbx is not None:
            mbx_rows.append({"timestamp": stamp, **mbx})
    result: dict[str, Any] = {"log_file": log_name, "log_size_bytes": len(raw), "log_sha256": hashlib.sha256(raw).hexdigest(), "pinned_action_refs": sorted(set(action_refs)), "cache_received_observations": received, "cache_sent_observations": sent, "mbx_object_cache_summaries": mbx_rows, "mise_download_progress": tool_rows, "cargo_download_package_lines": cargo_rows}
    if phase in {"cargo_source_restore", "mbx_object_restore", "mbx_bundle_restore", "other_cache_step"}:
        result["cache_bytes_received"] = max((row["received_bytes"] for row in received), default=None)
        result["cache_transfer_duration_ms"] = None
        result["cache_transfer_duration_unknown_reason"] = "GitHub API gives the whole step span; cache log has cumulative receive counters but no isolated transfer start/end pair."
    if phase in {"cargo_source_save", "mbx_bundle_save", "mbx_object_post_upload"}:
        result["cache_bytes_uploaded"] = max((row["uploaded_bytes"] for row in sent), default=None)
        result["cache_transfer_duration_ms"] = None
        result["cache_transfer_duration_unknown_reason"] = "GitHub API gives the whole step span; cache log has cumulative sent counters but no isolated transfer start/end pair."
    return result


def validate_identity(run: dict[str, Any], jobs_doc: dict[str, Any], repository: str) -> list[dict[str, Any]]:
    run_id = run.get("id")
    attempt = run.get("run_attempt")
    sha = run.get("head_sha")
    if not isinstance(run_id, int) or not isinstance(attempt, int) or not isinstance(sha, str):
        fail("run metadata lacks numeric id/attempt or head_sha")
    jobs = jobs_doc.get("jobs")
    if not isinstance(jobs, list) or jobs_doc.get("total_count") != len(jobs):
        fail("jobs API page is incomplete or malformed")
    if not jobs:
        fail("jobs API contains no jobs")
    expected_api = f"https://api.github.com/repos/{repository}/actions/runs/{run_id}"
    expected_html = f"https://github.com/{repository}/actions/runs/{run_id}"
    job_ids: set[int] = set()
    for job in jobs:
        if job.get("run_id") != run_id or job.get("run_attempt") != attempt or job.get("head_sha") != sha:
            fail(f"job {job.get('id')} belongs to a different run/attempt/source")
        if job.get("run_url") != expected_api or not str(job.get("html_url", "")).startswith(expected_html + "/job/"):
            fail(f"job {job.get('id')} URL does not bind repository and run")
        if not isinstance(job.get("id"), int) or job["id"] in job_ids:
            fail("duplicate or malformed job id")
        job_ids.add(job["id"])
    return jobs


def workflow_receipt(workflow_json: Path | None, workflow_file: Path | None, run: dict[str, Any], repository: str) -> dict[str, Any]:
    if workflow_json is None or workflow_file is None:
        return {"status": "unavailable", "reason": "Both contents API response and exact workflow bytes are required."}
    api = load_json(workflow_json)
    payload = workflow_file.read_bytes()
    if api.get("path") != run.get("path"):
        fail("contents API workflow path differs from run workflow path")
    expected_url = f"https://api.github.com/repos/{repository}/contents/{run['path']}?ref={run['head_sha']}"
    if api.get("url") != expected_url:
        fail("workflow contents API URL does not bind repository and exact run head SHA")
    if api.get("encoding") != "base64":
        fail("workflow contents response is not base64 encoded")
    try:
        content = base64.b64decode(api.get("content", ""), validate=False)
    except ValueError as error:
        fail(f"cannot decode workflow contents: {error}")
    if content != payload:
        fail("downloaded workflow bytes differ from contents API response")
    blob_sha = hashlib.sha1(b"blob " + str(len(payload)).encode() + b"\0" + payload).hexdigest()
    if blob_sha != api.get("sha"):
        fail("workflow bytes do not match contents API Git blob SHA")
    used = []
    for number, line in enumerate(payload.decode("utf-8").splitlines(), 1):
        match = re.match(r"^\s*(?:-\s*)?uses:\s*([^\s#]+)", line)
        if match:
            ref = match.group(1)
            used.append({"line": number, "ref": ref, "full_sha_pinned": bool(re.search(r"@[0-9a-f]{40}$", ref))})
    return {"status": "verified", "path": api["path"], "git_blob_sha": blob_sha, "api_blob_sha": api["sha"], "size_bytes": len(payload), "sha256": hashlib.sha256(payload).hexdigest(), "workflow_action_refs": used, "job_graph": parse_workflow_job_graph(payload), "source_receipts": [file_receipt(workflow_json, "workflow_contents_api"), file_receipt(workflow_file, "workflow_bytes")]}


def validate_artifact(artifact_json: Path | None, artifact_id: int | None, artifact_zip: Path | None, run: dict[str, Any], repository: str) -> tuple[dict[str, Any] | None, dict[str, Any] | None]:
    if artifact_json is None and artifact_zip is None and artifact_id is None:
        return None, None
    if artifact_json is None or artifact_zip is None or artifact_id is None:
        fail("artifact API response, artifact ID, and artifact ZIP must be supplied together")
    doc = load_json(artifact_json)
    artifacts = doc.get("artifacts")
    if not isinstance(artifacts, list) or doc.get("total_count") != len(artifacts):
        fail("artifact API response is incomplete or malformed")
    matches = [item for item in artifacts if item.get("id") == artifact_id]
    if len(matches) != 1:
        fail("requested artifact ID is absent or duplicated")
    item = matches[0]
    expected_name = f"velnor-final-r{run['id']}-a{run['run_attempt']}"
    binding = item.get("workflow_run") or {}
    expected_url = f"https://api.github.com/repos/{repository}/actions/artifacts/{artifact_id}"
    if item.get("name") != expected_name or item.get("url") != expected_url or binding.get("id") != run["id"] or binding.get("head_sha") != run["head_sha"]:
        fail("artifact metadata does not bind name, repository, run, attempt, and source SHA")
    receipt = file_receipt(artifact_zip, "final_report_artifact_zip")
    if item.get("size_in_bytes") != receipt["size_bytes"] or item.get("digest") != f"sha256:{receipt['sha256']}":
        fail("final report artifact size/digest differs from the artifact API")
    with zipfile.ZipFile(artifact_zip) as archive:
        if "final-report.json" not in archive.namelist():
            fail("final report artifact has no final-report.json")
        report_payload = archive.read("final-report.json")
    report = json.loads(report_payload)
    if report.get("run_key") != f"r{run['id']}-a{run['run_attempt']}" or report.get("report_id") != f"final-r{run['id']}-a{run['run_attempt']}":
        fail("final report payload does not bind to the requested run attempt")
    summary = {"artifact_id": artifact_id, "artifact_name": item["name"], "workflow_run_id": binding.get("id"), "head_sha": binding.get("head_sha"), "api_digest": item["digest"], "archive_size_bytes": receipt["size_bytes"], "artifact_api_receipt": file_receipt(artifact_json, "actions_artifacts_api"), "archive_receipt": receipt, "report_payload_receipt": {"file_name": "final-report.json", "size_bytes": len(report_payload), "sha256": hashlib.sha256(report_payload).hexdigest()}, "report_counts": report.get("counts"), "report_status": report.get("status")}
    return doc, summary


def inspect_task_reports(report_dir: Path | None, expected_run: int, expected_attempt: int, expected_sha: str, artifact_doc: dict[str, Any] | None) -> dict[str, Any]:
    if report_dir is None:
        return {"status": "unavailable", "reason": "No extracted task report directory was supplied."}
    reports = sorted(report_dir.rglob("task-*.json"))
    if not reports:
        fail("task report directory contains no task-*.json files")
    artifact_map = {item.get("name"): item for item in (artifact_doc or {}).get("artifacts", [])}
    receipts = []
    all_unmeasured = True
    value_counts = {field: {"zero": 0, "null": 0, "nonzero": 0} for field in STAGE_TIMINGS}
    for path in reports:
        doc = load_json(path)
        if doc.get("run_key") != f"r{expected_run}-a{expected_attempt}":
            fail(f"task report {path.name} is not bound to the requested run attempt")
        if artifact_doc is not None:
            artifact_name = path.relative_to(report_dir).parts[0]
            parent = artifact_map.get(artifact_name)
            binding = (parent or {}).get("workflow_run") or {}
            if parent is None or not artifact_name.startswith(f"velnor-crate-r{expected_run}-a{expected_attempt}-") or binding.get("id") != expected_run or binding.get("head_sha") != expected_sha:
                fail(f"task report parent artifact {artifact_name} is not bound to the requested run attempt and source")
        timing = doc.get("timing")
        for field in STAGE_TIMINGS:
            value = timing.get(field) if isinstance(timing, dict) else None
            if value is None:
                value_counts[field]["null"] += 1
            elif value == 0:
                value_counts[field]["zero"] += 1
            else:
                value_counts[field]["nonzero"] += 1
                all_unmeasured = False
        receipts.append(file_receipt(path, "legacy_task_report"))
    manifest = json.dumps(receipts, sort_keys=True, separators=(",", ":")).encode()
    return {"status": "unavailable_as_phase_measurement" if all_unmeasured else "not_used_as_hosted_phase_measurement", "report_count": len(reports), "stage_fields": list(STAGE_TIMINGS), "all_stage_values_zero_or_null": all_unmeasured, "stage_field_value_counts": value_counts, "parent_artifact_api_bound": artifact_doc is not None, "parent_artifact_attempt_name_bound": artifact_doc is not None, "parent_artifact_zip_digests_verified": False, "parent_artifact_zip_limit": "Extracted report files are hashed; the separate per-crate artifact ZIP bytes were not retained in this packet.", "zero_values_disposition": "zero and null sentinel fields are missing telemetry, not measured durations; task_ms is not split into phases", "source_manifest_sha256": hashlib.sha256(manifest).hexdigest(), "report_receipts": receipts}

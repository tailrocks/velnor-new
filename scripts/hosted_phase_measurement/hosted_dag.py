"""Strict parser and observed-path calculation for rendered GitHub job DAGs."""

from __future__ import annotations

import hashlib
import json
import re
from datetime import datetime, timezone
from typing import Any


def _time(value: str) -> datetime:
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        raise ValueError("timestamp has no timezone")
    return parsed.astimezone(timezone.utc)


def _ms(start: str, end: str) -> int:
    delta = (_time(end) - _time(start)).total_seconds()
    if delta < 0:
        raise ValueError("job end precedes start")
    return int(delta * 1000)


def _unavailable(reason: str) -> dict[str, Any]:
    return {"status": "unavailable", "reason": reason, "nodes": [], "sha256": None}


def _needs_value(value: str, lines: list[str], index: int, stop: int) -> list[str]:
    value = value.split("#", 1)[0].strip()
    if not value:
        found = []
        for line in lines[index + 1:stop]:
            if not line.strip():
                continue
            if len(line) - len(line.lstrip(" ")) < 6:
                break
            match = re.match(r"^\s{6}-\s*([A-Za-z0-9_-]+)\s*$", line)
            if match:
                found.append(match.group(1))
            else:
                raise ValueError("unsupported block-form needs entry")
        return found
    if value.startswith("[") and value.endswith("]"):
        pieces = [part.strip().strip("\"'") for part in value[1:-1].split(",") if part.strip()]
        if any(not re.fullmatch(r"[A-Za-z0-9_-]+", item) for item in pieces):
            raise ValueError("unsupported inline needs entry")
        return pieces
    value = value.strip("\"'")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", value):
        raise ValueError("unsupported scalar needs entry")
    return [value]


def parse_workflow_job_graph(payload: bytes) -> dict[str, Any]:
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError:
        return _unavailable("workflow source is not UTF-8")
    lines = text.splitlines()
    jobs_line = next((i for i, line in enumerate(lines) if line == "jobs:"), None)
    if jobs_line is None:
        return _unavailable("workflow has no top-level jobs mapping")
    starts: list[tuple[int, str]] = []
    for index in range(jobs_line + 1, len(lines)):
        line = lines[index]
        if line and not line.startswith(" ") and not line.startswith("#"):
            break
        match = re.match(r"^  ([A-Za-z0-9_-]+):\s*(?:#.*)?$", line)
        if match:
            starts.append((index, match.group(1)))
    if not starts:
        return _unavailable("workflow jobs mapping has no supported job keys")
    nodes = []
    try:
        for position, (start, job_id) in enumerate(starts):
            stop = starts[position + 1][0] if position + 1 < len(starts) else len(lines)
            display_name = None
            name_seen = False
            needs_seen = False
            needs: list[str] = []
            for index in range(start + 1, stop):
                line = lines[index]
                if len(line) - len(line.lstrip(" ")) != 4:
                    continue
                name_match = re.match(r"^    name:\s*(.*?)\s*$", line)
                if name_match:
                    if name_seen:
                        raise ValueError(f"job {job_id} repeats its display name key")
                    name_seen = True
                    display_name = name_match.group(1).strip().strip("\"'")
                need_match = re.match(r"^    needs:\s*(.*?)\s*$", line)
                if need_match:
                    if needs_seen:
                        raise ValueError(f"job {job_id} repeats its needs key")
                    needs_seen = True
                    needs = _needs_value(need_match.group(1), lines, index, stop)
            if not display_name:
                return _unavailable(f"job {job_id} has no explicit display name")
            if "${{" in display_name:
                return _unavailable(f"job {job_id} has a dynamic display name that cannot be joined exactly")
            nodes.append({"job_id": job_id, "display_name": display_name, "needs": needs})
        ids = {node["job_id"] for node in nodes}
        if len(ids) != len(nodes):
            return _unavailable("workflow contains duplicate job IDs")
        names = [node["display_name"] for node in nodes]
        if len(set(names)) != len(names):
            return _unavailable("workflow contains duplicate display names")
        for node in nodes:
            if len(set(node["needs"])) != len(node["needs"]):
                return _unavailable(f"job {node['job_id']} repeats a dependency")
            missing = sorted(set(node["needs"]) - ids)
            if missing:
                return _unavailable(f"job {node['job_id']} needs missing workflow jobs: {missing}")
        remaining = set(ids)
        visited = set()
        while remaining:
            ready = {node["job_id"] for node in nodes if node["job_id"] in remaining and set(node["needs"]) <= visited}
            if not ready:
                return _unavailable("workflow job needs graph contains a cycle")
            visited |= ready
            remaining -= ready
        canonical = json.dumps(nodes, sort_keys=True, separators=(",", ":")).encode()
        return {"status": "verified", "nodes": nodes, "sha256": hashlib.sha256(canonical).hexdigest(), "parser_scope": "static job IDs, explicit names, scalar/list needs only"}
    except ValueError as error:
        return _unavailable(str(error))


def _path_unavailable(reason: str) -> dict[str, Any]:
    return {"status": "unavailable", "duration_ms": None, "reason": reason}


def _matched_api_jobs(graph: dict[str, Any], jobs: list[dict[str, Any]]) -> tuple[dict[str, dict[str, Any]], str | None]:
    nodes = graph["nodes"]
    by_name = {job["name"]: job for job in jobs}
    if len(by_name) != len(jobs) or len(nodes) != len(jobs):
        return {}, "workflow job names do not map one-to-one to API jobs"
    matched = {}
    for node in nodes:
        job_id = node["job_id"]
        api_job = by_name.get(node["display_name"])
        if api_job is None:
            return {}, f"workflow job {job_id} has no exact API display-name match"
        if api_job.get("conclusion") != "success":
            return {}, f"workflow job {job_id} did not complete successfully"
        if not api_job.get("started_at") or not api_job.get("completed_at"):
            return {}, f"workflow job {job_id} lacks exact API start/end timestamps"
        matched[job_id] = api_job
    return matched, None


def _step_join_error(nodes: list[dict[str, Any]], api_jobs: dict[str, dict[str, Any]], step_rows: list[dict[str, Any]]) -> str | None:
    steps_by_job: dict[int, list[dict[str, Any]]] = {}
    for step in step_rows:
        steps_by_job.setdefault(step["job_id"], []).append(step)
    for node in nodes:
        job_id = node["job_id"]
        job = api_jobs[job_id]
        active = [row for row in steps_by_job.get(job["id"], []) if row["conclusion"] != "skipped"]
        if not active or any(not _has_log_join(step) for step in active):
            return f"workflow job {job_id} lacks a complete API-step/raw-log join"
        error = _step_span_error(job_id, job, active)
        if error:
            return error
    return None


def _has_log_join(step: dict[str, Any]) -> bool:
    return step.get("status") == "completed" and step.get("duration_ms_from_api_timestamps") is not None and step.get("log_evidence") is not None


def _step_span_error(job_id: str, job: dict[str, Any], active: list[dict[str, Any]]) -> str | None:
    job_start, job_end = _time(job["started_at"]), _time(job["completed_at"])
    prev_end = None
    for step in sorted(active, key=lambda row: row["step_number"] or 0):
        start, end = step.get("started_at"), step.get("completed_at")
        if not start or not end:
            return f"workflow job {job_id} has a completed step without timestamps"
        start_at, end_at = _time(start), _time(end)
        if _ms(start, end) != step["duration_ms_from_api_timestamps"]:
            return f"workflow job {job_id} step duration differs from its API timestamps"
        if (start_at - job_start).total_seconds() < -1 or (job_end - end_at).total_seconds() < -1:
            return f"workflow job {job_id} step timestamps fall outside the API job span"
        if prev_end is not None and (prev_end - start_at).total_seconds() > 1:
            return f"workflow job {job_id} step timestamps overlap beyond API precision"
        prev_end = end_at
    return None


def _topological_order(nodes: list[dict[str, Any]]) -> list[str] | None:
    node_by_id = {node["job_id"]: node for node in nodes}
    remaining = set(node_by_id)
    visited: set[str] = set()
    order = []
    while remaining:
        ready = sorted(job_id for job_id in remaining if set(node_by_id[job_id]["needs"]) <= visited)
        if not ready:
            return None
        order.extend(ready)
        visited.update(ready)
        remaining.difference_update(ready)
    return order


def _score_job_paths(order: list[str], nodes: list[dict[str, Any]], api_jobs: dict[str, dict[str, Any]]) -> tuple[dict[str, dict[str, Any]], dict[str, dict[str, dict[str, Any]]], dict[str, str], str | None]:
    node_by_id = {node["job_id"]: node for node in nodes}
    best: dict[str, dict[str, Any]] = {}
    offsets: dict[str, dict[str, dict[str, Any]]] = {}
    predecessors: dict[str, str] = {}
    for job_id in order:
        node, job = node_by_id[job_id], api_jobs[job_id]
        start, end = job["started_at"], job["completed_at"]
        own = _ms(start, end)
        needs = sorted(node["needs"])
        if not needs:
            best[job_id] = {"duration_ms": own, "path": [job_id], "unclassified_post_dependency_ready_wait_ms": 0}
            offsets[job_id] = {}
            continue
        latest_end = max(_time(api_jobs[parent]["completed_at"]) for parent in needs)
        latest = [parent for parent in needs if _time(api_jobs[parent]["completed_at"]) == latest_end]
        parent_id = min(latest, key=lambda parent: (-best[parent]["duration_ms"], parent))
        ready_gap = int((_time(start) - latest_end).total_seconds() * 1000)
        if ready_gap < 0:
            return {}, {}, {}, f"API job {job['id']} started before its latest dependency completed"
        parent_path = best[parent_id]
        best[job_id] = {"duration_ms": parent_path["duration_ms"] + ready_gap + own, "path": parent_path["path"] + [job_id], "unclassified_post_dependency_ready_wait_ms": parent_path["unclassified_post_dependency_ready_wait_ms"] + ready_gap}
        predecessors[job_id] = parent_id
        offsets[job_id] = _dependency_offsets(needs, latest, parent_id, job, api_jobs)
    return best, offsets, predecessors, None


def _dependency_offsets(needs: list[str], latest: list[str], chosen: str, job: dict[str, Any], api_jobs: dict[str, dict[str, Any]]) -> dict[str, dict[str, Any]]:
    start = _time(job["started_at"])
    return {
        dependency: {
            "parent_completed_at": api_jobs[dependency]["completed_at"],
            "child_started_at_minus_parent_completion_ms": int((start - _time(api_jobs[dependency]["completed_at"])).total_seconds() * 1000),
            "latest_completed_parent": dependency in latest,
            "selected_critical_path_predecessor": dependency == chosen,
        }
        for dependency in needs
    }


def _select_final_job(nodes: list[dict[str, Any]], best: dict[str, dict[str, Any]]) -> str:
    parents = {dependency for node in nodes for dependency in node["needs"]}
    sinks = sorted(node["job_id"] for node in nodes if node["job_id"] not in parents)
    return min(sinks, key=lambda job_id: (-best[job_id]["duration_ms"], job_id))


def _path_result(final_job: str, best: dict[str, dict[str, Any]], offsets: dict[str, dict[str, dict[str, Any]]], predecessors: dict[str, str], api_jobs: dict[str, dict[str, Any]], step_rows: list[dict[str, Any]]) -> dict[str, Any]:
    chosen = best[final_job]
    path = chosen["path"]
    path_jobs = [api_jobs[job_id] for job_id in path]
    path_api_ids = {job["id"] for job in path_jobs}
    path_steps = [row for row in step_rows if row["job_id"] in path_api_ids]
    joined_steps = [row for row in path_steps if row["conclusion"] != "skipped" and row["log_evidence"] is not None]
    return {
        "status": "verified", "duration_ms": chosen["duration_ms"], "reason": None,
        "source_job_ids": path, "api_job_ids": [job["id"] for job in path_jobs],
        "display_names": [job["name"] for job in path_jobs],
        "start_at": path_jobs[0]["started_at"], "end_at": path_jobs[-1]["completed_at"],
        "unclassified_post_dependency_ready_wait_ms": chosen["unclassified_post_dependency_ready_wait_ms"],
        "dependency_gap_method": "For tied latest dependency completions, use the greatest accumulated path duration, then ascending source job ID. Add only child start minus latest completion as unclassified wait.",
        "critical_predecessor_by_source_job": {key: predecessors[key] for key in path[1:]},
        "dependency_completion_offsets_ms_by_source_job": {key: offsets[key] for key in path[1:]},
        "path_api_step_count": len(path_steps), "joined_api_step_count": len(joined_steps),
        "skipped_api_step_count": sum(1 for row in path_steps if row["conclusion"] == "skipped"),
        "raw_step_logs_joined": True, "queue_duration_ms": None,
        "queue_unknown_reason": "API job timestamps do not split runner queue/provision from scheduling; time from the latest dependency completion to job start is an unclassified wait.",
    }


def compute_workflow_dag_path(graph: dict[str, Any], jobs: list[dict[str, Any]], step_rows: list[dict[str, Any]], missing_step_logs: list[dict[str, Any]]) -> dict[str, Any]:
    if graph.get("status") != "verified":
        return _path_unavailable(graph.get("reason", "workflow DAG is not verified"))
    if missing_step_logs:
        return _path_unavailable("one or more completed step logs did not join to the exact API step identity")
    nodes = graph["nodes"]
    api_jobs, error = _matched_api_jobs(graph, jobs)
    if error:
        return _path_unavailable(error)
    error = _step_join_error(nodes, api_jobs, step_rows)
    if error:
        return _path_unavailable(error)
    order = _topological_order(nodes)
    if order is None:
        return _path_unavailable("workflow DAG could not be ordered")
    best, offsets, predecessors, error = _score_job_paths(order, nodes, api_jobs)
    if error:
        return _path_unavailable(error)
    final_job = _select_final_job(nodes, best)
    return _path_result(final_job, best, offsets, predecessors, api_jobs, step_rows)

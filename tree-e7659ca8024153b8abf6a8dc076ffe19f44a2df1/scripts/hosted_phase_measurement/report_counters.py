"""Summarize counters actually present in exact joined step logs."""

from __future__ import annotations

from typing import Any


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

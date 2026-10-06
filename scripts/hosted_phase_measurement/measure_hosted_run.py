#!/usr/bin/env python3
"""Collect source-bound hosted GitHub Actions phase evidence from retained API data."""

from __future__ import annotations

import argparse
import json
import sys
import zipfile
from pathlib import Path

from .hosted_dag import compute_workflow_dag_path, parse_workflow_job_graph
from .measure_support import (SCHEMA, STAGE_TIMINGS, UNKNOWN_ACTUAL_COUNTER_REASON_FIELDS,
                              duration_ms, extract_log_evidence, inspect_task_reports,
                              read_step_logs,
                              validate_actual_counter_unknowns, validate_artifact,
                              validate_identity, workflow_receipt)
from .report_builder import build_report
from .step_phases import STEP_PHASES, normalize_step_name, phase_for_step


def cli() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True, help="owner/repo from the API request path")
    parser.add_argument("--run-json", type=Path, required=True)
    parser.add_argument("--jobs-json", type=Path, required=True)
    parser.add_argument("--logs-zip", type=Path, required=True)
    parser.add_argument("--workflow-json", type=Path)
    parser.add_argument("--workflow-file", type=Path)
    parser.add_argument("--artifact-json", type=Path)
    parser.add_argument("--artifact-id", type=int)
    parser.add_argument("--final-artifact", type=Path)
    parser.add_argument("--task-report-dir", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    try:
        report = build_report(args)
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    except (OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
        print(f"measurement collection failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(cli())

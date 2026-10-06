"""Exact normalized GitHub Actions step-name to phase mapping."""

from __future__ import annotations

import re


STEP_PHASES = {
    "set up job": "runner_setup",
    "checkout": "checkout",
    "post checkout": "checkout_post",
    "setup mise": "mise_setup",
    "prepare pinned tools": "tool_provision",
    "prepare rust components": "rust_component_provision",
    "restore cargo sources": "cargo_source_restore",
    "save cargo sources": "cargo_source_save",
    "restore mbx objects": "mbx_object_restore",
    "post restore mbx objects": "mbx_object_post_upload",
    "restore mbx single bundle": "mbx_bundle_restore",
    "save mbx single bundle": "mbx_bundle_save",
    "import mbx single bundle": "mbx_bundle_import",
    "export mbx single bundle": "mbx_bundle_export",
    "fetch cargo sources": "cargo_fetch",
    "fetch cargo sources crates velnor runner cargo toml": "cargo_fetch",
    "check cargo sources": "cargo_source_check",
    "check generated files": "generated_files_check",
    "prepare mbx bundle key": "mbx_bundle_key",
    "format": "format",
    "clippy": "clippy",
    "build test executables": "test_build",
    "unit and integration tests": "unit_integration_tests",
    "doctests": "doctests",
    "documentation": "documentation",
    "upload crate reports": "report_upload",
    "download every expected matrix artifact": "report_download",
    "merge reports": "report_merge",
    "plan": "plan",
    "publish plan": "plan_publish",
    "publish final report": "final_report_publish",
    "publish baseline": "baseline_publish",
    "upload baseline": "baseline_upload",
}


def normalize_step_name(name: str) -> str:
    """Normalize case and punctuation while preserving exact whole-name matching."""
    return " ".join(re.findall(r"[a-z0-9]+", name.casefold()))


def phase_for_step(name: str) -> str:
    normalized = normalize_step_name(name)
    return STEP_PHASES.get(normalized, "other_cache_step" if "cache" in normalized else "other")

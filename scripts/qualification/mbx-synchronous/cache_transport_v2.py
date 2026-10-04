"""Strict public cache transport observations; V2 export has no usefulness authority."""
import json
import re
from typing import Any

MAX_BYTES = 8 * 1024 * 1024
MAX_U64 = (1 << 64) - 1
CAPTURE_REASON = {
    "captured": "scheduler_validity_not_proven",
    "unavailable_owner_coverage": "owner_coverage_unavailable",
    "unavailable_owner_proof": "owner_proof_unavailable",
    "unavailable_managed_overlap": "managed_overlap",
}
CAPTURED_QUALIFICATION = (
    "compiled actions, predictions and Cargo unit state; includes Cargo output and intermediate roots; "
    "excludes only effective build-root compiler-query cache .rustc_info.json; other scheduler content "
    "differences are reported, not proven additional cache hits"
)
UNAVAILABLE_QUALIFICATION = (
    "compiled actions and predictions; Cargo workspace capture unavailable; "
    "see workspace_capture_unavailable_reason; workspace persistence not verified"
)
BUDGET_QUALIFICATION = "verified inventory exceeds owner budget; persistence not verified"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def keys(value: Any, fields: str, name: str) -> None:
    require(type(value) is dict and set(value) == set(fields.split()), name + " schema differs")


def unsigned(value: Any, name: str) -> None:
    require(type(value) is int and 0 <= value <= MAX_U64, name + " unsigned integer required")


def boolean(value: Any, name: str) -> None:
    require(type(value) is bool, name + " boolean required")


def version(value: Any, expected: int) -> None:
    require(type(value) is int and value == expected, "unsupported native transport version")


def object_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate native transport field")
        result[key] = value
    return result


def export(value: dict[str, Any]) -> None:
    version(value.get("version"), 2)
    boolean(value.get("budget_refused"), "budget_refused")
    if value["budget_refused"]:
        keys(value, "version exported budget_refused snapshot_budget_bytes logical_closure_bytes qualification", "budget refusal")
        require(value["exported"] is False and value["qualification"] == BUDGET_QUALIFICATION,
                "invalid budget refusal observation")
        unsigned(value["snapshot_budget_bytes"], "snapshot_budget_bytes")
        unsigned(value["logical_closure_bytes"], "logical_closure_bytes")
        return
    keys(value, "version budget_refused snapshot_budget_bytes exported actions objects bytes "
         "emitted_bundle_useful_delta workspace_usefulness delta semantic_digest workspace_comparison "
         "workspace_comparison_exclusions workspace_transport_scope workspace_capture "
         "workspace_capture_unavailable_reason workspace_persistence_verified qualification", "V2 export")
    for name in ("exported", "emitted_bundle_useful_delta"):
        boolean(value[name], name)
    for name in ("actions", "objects", "bytes"):
        unsigned(value[name], name)
    if value["snapshot_budget_bytes"] is not None:
        unsigned(value["snapshot_budget_bytes"], "snapshot_budget_bytes")
    delta = value["delta"]
    if delta is not None:
        keys(delta, "new_action_results changed_action_results new_predictions new_workspace_variants "
             "changed_workspace_variants", "emitted bundle delta")
        for name, count in delta.items():
            unsigned(count, name)
    keys(value["workspace_usefulness"], "status reason", "workspace usefulness")
    require(value["workspace_usefulness"]["status"] == "unavailable" and
            value["workspace_usefulness"]["reason"] in CAPTURE_REASON.values(), "workspace usefulness unavailable only")
    capture = value["workspace_capture"]
    require(type(capture) is str and capture in CAPTURE_REASON and
            value["workspace_usefulness"]["reason"] == CAPTURE_REASON[capture], "capture/usefulness reason differs")
    reason = value["workspace_capture_unavailable_reason"]
    require(reason is None or type(reason) is str, "capture diagnostic string or null required")
    require(type(value["semantic_digest"]) is str and re.fullmatch("[0-9a-f]{64}", value["semantic_digest"]),
            "semantic digest differs")
    require(value["workspace_comparison"] == "relative_path_type_content_mode_symlink_target" and
            value["workspace_comparison_exclusions"] == ["effective_build_root/.rustc_info.json"] and
            value["workspace_transport_scope"] == "recorded_target_and_build_directories",
            "native workspace transport scope differs")
    require(value["workspace_persistence_verified"] is False, "parsed workspace persistence authority forbidden")
    expected = CAPTURED_QUALIFICATION if capture == "captured" else UNAVAILABLE_QUALIFICATION
    require(value["qualification"] == expected, "native qualification description differs")
    # Delta omits reusable_workspace_variants; never infer usefulness or scheduler validity from its counts.


def imported(value: dict[str, Any]) -> None:
    keys(value, "version actions objects bytes comparison_state_recorded workspace_restored workspace_restore", "native import")
    version(value["version"], 1)
    for name in ("actions", "objects", "bytes"):
        unsigned(value[name], name)
    for name in ("comparison_state_recorded", "workspace_restored"):
        boolean(value[name], name)
    statuses = {"not_present", "restored", "skipped_ambiguous", "skipped_managed_overlap",
                "skipped_incompatible", "skipped_unavailable", "skipped_nonempty", "failed", "metadata_unavailable"}
    require(type(value["workspace_restore"]) is str and value["workspace_restore"] in statuses,
            "unknown native restore status")
    require(value["workspace_restored"] == (value["workspace_restore"] == "restored"),
            "native workspace restore observation differs")


def comparison(value: dict[str, Any]) -> None:
    keys(value, "version valid empty", "native comparison")
    version(value["version"], 1)
    require(value["valid"] is True, "comparison owner validation unavailable")
    boolean(value["empty"], "empty")


def validate(operation: str, raw: bytes) -> dict[str, Any]:
    require(type(raw) is bytes and len(raw) <= MAX_BYTES, "native transport report exceeds bound")
    def nonfinite(value: str) -> None:
        raise ValueError("nonfinite native transport value: " + value)
    value = json.loads(raw, object_pairs_hook=object_pairs, parse_constant=nonfinite)
    require(type(value) is dict, "native transport object required")
    validators = {"export": export, "import": imported, "comparison-state": comparison}
    require(operation in validators, "unsupported native transport operation")
    validators[operation](value)
    return value

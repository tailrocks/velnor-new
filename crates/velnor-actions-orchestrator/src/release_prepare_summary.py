"""Pinned release-plz 0.3.169 update summary, bound to planned byte changes."""
import re

_SUMMARY_ROW = re.compile(r"\* `([A-Za-z0-9][A-Za-z0-9_-]{0,63})`: (.+)")
_SUMMARY_BREAK = re.compile(r"\n### ⚠️ `([A-Za-z0-9][A-Za-z0-9_-]{0,63})` breaking changes\n\n```")
_SUMMARY_STATUS = {" (✓ API compatible changes)": "compatible",
                   " (⚠️ API breaking changes)": "incompatible"}


def preparation_summary(stdout, planned, approved_versions):
    """Parse only exact upstream rows/reports; never infer compatibility from presence."""
    require(isinstance(stdout, bytes) and len(stdout) <= 2 * 1024 * 1024,
            "preparation_summary_size")
    text = stdout.decode("utf-8")
    blocks = list(_SUMMARY_BREAK.finditer(text))
    rows = text[:blocks[0].start()] if blocks else text
    result = {}
    for line in rows.splitlines():
        if not line:
            continue
        match = _SUMMARY_ROW.fullmatch(line)
        require(match is not None, "preparation_summary_row")
        name, transition = match.groups()
        require(name in planned and name not in result, "preparation_summary_package")
        status = "skipped"
        for suffix, outcome in _SUMMARY_STATUS.items():
            if transition.endswith(suffix):
                transition, status = transition[:-len(suffix)], outcome
                break
        parts = transition.split(" -> ")
        require(len(parts) in (1, 2), "preparation_summary_transition")
        previous, version = parts[0], parts[-1]
        _require_version(previous)
        _require_version(version)
        require(version == planned[name]["version"], "preparation_summary_version")
        if len(parts) == 1:
            require(status == "skipped", "preparation_summary_equal_status")
            status = "unknown"
        result[name] = {"previous_version": previous, "semver_check": status,
                        "breaking_changes": ""}
    for index, match in enumerate(blocks):
        name = match.group(1)
        require(name in result and not result[name]["breaking_changes"],
                "preparation_summary_breaking_package")
        end = blocks[index + 1].start() if index + 1 < len(blocks) else len(text)
        block = text[match.end():end].rstrip("\n")
        require(block.endswith("```") and len(block) > 3, "preparation_summary_breaking_block")
        report = block[:-3]
        require(len(report) <= 32768 and result[name]["semver_check"] in
                ("incompatible", "unknown"), "preparation_summary_breaking_status")
        result[name]["semver_check"] = "incompatible"
        result[name]["breaking_changes"] = report
    for name, package in planned.items():
        if name not in result:
            require(package["version"] == approved_versions[name] and not package["notes"],
                    "preparation_summary_missing_update")
            result[name] = {"previous_version": approved_versions[name],
                            "semver_check": "unknown", "breaking_changes": ""}
        require(result[name]["semver_check"] != "incompatible" or
                result[name]["breaking_changes"], "preparation_summary_missing_breaking")
    return result

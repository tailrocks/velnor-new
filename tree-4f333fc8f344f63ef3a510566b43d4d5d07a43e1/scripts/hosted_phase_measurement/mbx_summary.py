"""Parse the pinned MBX object-cache summary without dropping marked rows."""

from __future__ import annotations

import re
from decimal import Decimal, InvalidOperation
from typing import Any


MBX_MARKER = re.compile(r"mbx\[cache\]:\s*object cache:", re.IGNORECASE)
SUMMARY = re.compile(
    r"mbx\[cache\]:\s*object cache:\s*(?P<lookups>[^;]*);\s*"
    r"(?P<downloaded>\d+(?:\.\d+)?)\s*(?P<download_unit>[A-Za-z]+)\s+downloaded,\s*"
    r"(?P<uploaded>\d+(?:\.\d+)?)\s*(?P<upload_unit>[A-Za-z]+)\s+uploaded,\s*"
    r"(?P<local>\d+(?:\.\d+)?)\s*(?P<local_unit>[A-Za-z]+)\s+stored locally\s*[.!]?\s*",
    re.IGNORECASE,
)
LOOKUP = re.compile(r"(?P<count>\d+)\s+(?P<label>hits?|miss(?:es)?|not\s+looked\s+up|bypassed)", re.IGNORECASE)
UNIT_BYTES = {
    "b": 1,
    "kb": 1_000,
    "kib": 1 << 10,
    "mb": 1_000_000,
    "mib": 1 << 20,
    "gb": 1_000_000_000,
    "gib": 1 << 30,
    "tb": 1_000_000_000_000,
    "tib": 1 << 40,
}
MIB = Decimal(1 << 20)


def _fail(message: str) -> None:
    raise ValueError(message)


def _lookup_counts(value: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    labels = {
        "hit": "hits", "hits": "hits", "miss": "misses", "misses": "misses",
        "not looked up": "not_looked_up", "bypassed": "bypassed",
    }
    for item in value.split(","):
        match = LOOKUP.fullmatch(item.strip())
        if match is None:
            _fail(f"malformed MBX lookup counter: {item!r}")
        label = labels[" ".join(match.group("label").lower().split())]
        if label in counts:
            _fail(f"duplicate MBX lookup counter: {label}")
        counts[label] = int(match.group("count"))
    if not counts:
        _fail("MBX summary has no lookup counters")
    return counts


def _byte_count(value: str, unit: str, label: str) -> int:
    multiplier = UNIT_BYTES.get(unit.lower())
    if multiplier is None:
        _fail(f"unsupported MBX {label} unit: {unit!r}")
    try:
        amount = Decimal(value) * multiplier
    except InvalidOperation as error:
        raise ValueError(f"invalid MBX {label} amount: {value!r}") from error
    if not amount.is_finite() or amount < 0 or amount != amount.to_integral_value():
        _fail(f"MBX {label} amount is not a nonnegative integer byte count")
    return int(amount)


def _local_store_mib(value: str, unit: str) -> float:
    multiplier = UNIT_BYTES.get(unit.lower())
    if multiplier is None:
        _fail(f"unsupported MBX local storage unit: {unit!r}")
    try:
        amount = Decimal(value) * multiplier / MIB
    except InvalidOperation as error:
        raise ValueError(f"invalid MBX local storage amount: {value!r}") from error
    if not amount.is_finite() or amount < 0:
        _fail("MBX local storage amount is not nonnegative")
    return float(amount)


def parse_mbx_summary(line: str) -> dict[str, Any] | None:
    """Return one summary; malformed lines carrying its marker fail visibly."""
    markers = list(MBX_MARKER.finditer(line))
    if not markers:
        return None
    if len(markers) != 1:
        _fail("line contains multiple MBX object-cache markers")
    match = SUMMARY.fullmatch(line[markers[0].start():].strip())
    if match is None:
        _fail(f"malformed MBX object-cache summary: {line!r}")
    downloaded = _byte_count(match.group("downloaded"), match.group("download_unit"), "download")
    uploaded = _byte_count(match.group("uploaded"), match.group("upload_unit"), "upload")
    return {
        "counts": _lookup_counts(match.group("lookups")),
        "remote_downloaded_bytes": downloaded,
        "remote_uploaded_bytes": uploaded,
        "local_store_mib": _local_store_mib(match.group("local"), match.group("local_unit")),
        "counter_scope": "one pinned MBX object-cache summary; does not measure total compiler freshness",
    }

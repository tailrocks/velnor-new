#!/usr/bin/env python3
"""Validate the real image's bounded numeric smoke response."""

import json
import pathlib
import sys


EXPECTED = {
    "schema_version",
    "docker_root_free_bytes",
    "docker_root_total_bytes",
    "memory_available_bytes",
    "load_milli",
    "memory_psi_some_avg10_bps",
}


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate JSON key")
        value[key] = item
    return value


def unsigned(value):
    return type(value) is int and value >= 0


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: validate_smoke.py OUTPUT ROOT")
    raw = pathlib.Path(sys.argv[1]).read_bytes()
    root = pathlib.Path(sys.argv[2])
    if len(raw) > 512 or not raw.endswith(b"\n") or raw.count(b"\n") != 1:
        raise ValueError("probe output is not one bounded newline-terminated record")
    record = json.loads(raw, object_pairs_hook=unique_object)
    if type(record) is not dict or set(record) != EXPECTED:
        raise ValueError("probe output has an unknown or missing field")
    canonical = (json.dumps(record, separators=(",", ":"), ensure_ascii=True) + "\n").encode()
    if raw != canonical:
        raise ValueError("probe output is not the canonical compact record")
    if record["schema_version"] != 1 or type(record["schema_version"]) is not int:
        raise ValueError("unsupported probe schema")
    for key in (
        "docker_root_free_bytes",
        "docker_root_total_bytes",
        "memory_available_bytes",
        "load_milli",
    ):
        if not unsigned(record[key]):
            raise ValueError(f"{key} is not an unsigned integer")
    if record["docker_root_free_bytes"] > record["docker_root_total_bytes"]:
        raise ValueError("Docker-root free bytes exceed total bytes")
    psi = record["memory_psi_some_avg10_bps"]
    if psi is not None and (not unsigned(psi) or psi > 10_000):
        raise ValueError("PSI value is outside its documented range")
    if not root.is_dir():
        raise ValueError("the smoke mount source is not a directory")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"resource-probe smoke validation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error

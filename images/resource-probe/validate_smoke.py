#!/usr/bin/env python3
"""Validate the real image's bounded numeric smoke response."""

import json
import pathlib
import sys


EXPECTED = {
    "schema_version",
    "docker_root_mount_target",
    "docker_root_mount_id",
    "docker_root_device_major",
    "docker_root_device_minor",
    "docker_root_statx_device_major",
    "docker_root_statx_device_minor",
    "docker_root_fs_type_magic",
    "docker_root_fs_type_name",
    "docker_root_filesystem_id",
    "guest_boot_id",
    "docker_root_free_bytes",
    "docker_root_total_bytes",
    "docker_root_available_bytes",
    "docker_root_inode_total",
    "docker_root_inode_free",
    "docker_root_inode_available",
    "docker_root_inode_policy",
    "memory_available_bytes",
    "load_milli",
    "memory_psi_some_avg10_bps",
    "probe_cgroup_path",
    "probe_cgroup_memory_max_bytes",
    "probe_cgroup_memory_current_bytes",
    "probe_cgroup_swap_max_bytes",
    "probe_cgroup_swap_current_bytes",
    "probe_cgroup_cpu_quota_micros",
    "probe_cgroup_cpu_period_micros",
    "probe_cgroup_pids_max",
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
    if len(raw) > 2048 or not raw.endswith(b"\n") or raw.count(b"\n") != 1:
        raise ValueError("probe output is not one bounded newline-terminated record")
    record = json.loads(raw, object_pairs_hook=unique_object)
    if type(record) is not dict or set(record) != EXPECTED:
        raise ValueError("probe output has an unknown or missing field")
    canonical = (json.dumps(record, separators=(",", ":"), ensure_ascii=True) + "\n").encode()
    if raw != canonical:
        raise ValueError("probe output is not the canonical compact record")
    if record["schema_version"] != 2 or type(record["schema_version"]) is not int:
        raise ValueError("unsupported probe schema")
    for key in (
        "docker_root_free_bytes",
        "docker_root_total_bytes",
        "docker_root_available_bytes",
        "docker_root_inode_total",
        "docker_root_inode_free",
        "docker_root_inode_available",
        "docker_root_mount_id",
        "docker_root_device_major",
        "docker_root_device_minor",
        "docker_root_statx_device_major",
        "docker_root_statx_device_minor",
        "docker_root_fs_type_magic",
        "docker_root_filesystem_id",
        "memory_available_bytes",
        "load_milli",
        "probe_cgroup_memory_current_bytes",
        "probe_cgroup_swap_current_bytes",
        "probe_cgroup_cpu_period_micros",
    ):
        if not unsigned(record[key]):
            raise ValueError(f"{key} is not an unsigned integer")
    if not (record["docker_root_available_bytes"] <= record["docker_root_free_bytes"] <= record["docker_root_total_bytes"]):
        raise ValueError("Docker-root free bytes exceed total bytes")
    if record["docker_root_inode_free"] > record["docker_root_inode_total"] or record["docker_root_inode_available"] > record["docker_root_inode_free"]:
        raise ValueError("Docker-root inode counts exceed total counts")
    policy = record["docker_root_inode_policy"]
    if policy not in ("finite", "dynamic_btrfs", "unknown"):
        raise ValueError("inode policy is unknown")
    inode_counts = (record["docker_root_inode_total"], record["docker_root_inode_free"], record["docker_root_inode_available"])
    filesystem_name = record["docker_root_fs_type_name"]
    if filesystem_name == "btrfs":
        if record["docker_root_fs_type_magic"] != 0x9123683E:
            raise ValueError("Btrfs type name does not match its type magic")
        expected_policy = "dynamic_btrfs" if inode_counts == (0, 0, 0) else "unknown"
        if policy != expected_policy:
            raise ValueError("Btrfs inode policy does not match its raw facts")
    elif (
        record["docker_root_device_major"] != record["docker_root_statx_device_major"]
        or record["docker_root_device_minor"] != record["docker_root_statx_device_minor"]
    ):
        raise ValueError("non-Btrfs mount and statx device identities differ")
    if filesystem_name != "btrfs" and policy == "dynamic_btrfs":
        raise ValueError("dynamic inode policy requires Btrfs")
    elif policy == "finite" and record["docker_root_inode_total"] == 0:
        raise ValueError("finite inode policy has no inode total")
    elif policy == "unknown" and record["docker_root_inode_total"] != 0:
        raise ValueError("unknown inode policy has a finite inode total")
    for key in ("probe_cgroup_memory_max_bytes", "probe_cgroup_swap_max_bytes", "probe_cgroup_cpu_quota_micros", "probe_cgroup_pids_max"):
        if record[key] is not None and not unsigned(record[key]):
            raise ValueError(f"{key} is not null or an unsigned integer")
    if type(record["docker_root_mount_target"]) is not str or not record["docker_root_mount_target"].startswith("/"):
        raise ValueError("Docker-root target is not an absolute path")
    if type(record["docker_root_fs_type_name"]) is not str or not record["docker_root_fs_type_name"]:
        raise ValueError("filesystem type name is empty")
    if type(record["guest_boot_id"]) is not str or len(record["guest_boot_id"]) != 36:
        raise ValueError("guest boot ID is malformed")
    if type(record["probe_cgroup_path"]) is not str or not record["probe_cgroup_path"].startswith("/"):
        raise ValueError("probe cgroup path is not absolute")
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

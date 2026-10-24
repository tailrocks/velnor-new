#!/usr/bin/env python3
"""Validate the real image's bounded numeric smoke response."""

import json
import pathlib
import sys
import unicodedata


EXPECTED = {
    "schema_version",
    "docker_root_mount_target",
    "docker_root_mount_source",
    "docker_root_mount_root",
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
    "probe_cgroup_cpuset_cpus_count",
    "cpu_ancestor_limits",
    "probe_host_pid",
    "probe_namespace_pid",
    "probe_cgroup_pids_max",
}

DOCKER_ROOT_TARGET = "/velnor/docker-root"
MAX_CGROUP_ANCESTORS = 16
MAX_EFFECTIVE_CPUS = 4096


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("The JSON object has a duplicate key.")
        value[key] = item
    return value


def unsigned(value):
    return type(value) is int and value >= 0


def has_ascii_controls(value):
    return any(byte < 0x20 or byte == 0x7F for byte in value.encode("utf-8"))


def valid_mount_source(value):
    return (
        type(value) is str
        and bool(value)
        and len(value.encode("utf-8")) <= 1024
        and not has_ascii_controls(value)
    )


def valid_mount_root(value):
    if type(value) is not str:
        return False
    if value == "/":
        return True
    encoded = value.encode("utf-8")
    if not value or len(encoded) > 1024 or not value.startswith("/") or has_ascii_controls(value):
        return False
    for component in value.split("/")[1:]:
        if not component or component in (".", ".."):
            return False
        bytes_component = component.encode("utf-8")
        index = 0
        while index < len(bytes_component):
            if bytes_component[index] == ord("\\"):
                if index + 3 >= len(bytes_component) or any(
                    byte < ord("0") or byte > ord("7")
                    for byte in bytes_component[index + 1 : index + 4]
                ):
                    return False
                index += 4
            else:
                index += 1
    return True


def cgroup_path(value):
    if (
        type(value) is not str
        or not value.startswith("/")
        or value == "/"
        or len(value.encode("utf-8")) > 256
        or "//" in value
        or any(character in "\\:" or unicodedata.category(character) == "Cc" for character in value)
    ):
        return None
    parts = [] if value == "/" else value[1:].split("/")
    if any(not part or part in (".", "..") for part in parts):
        return None
    if len(parts) > MAX_CGROUP_ANCESTORS:
        return None
    return parts


def expected_ancestor_paths(parts):
    paths = ["/"]
    current = ""
    for part in parts[:-1]:
        current += "/" + part
        paths.append(current)
    return paths


def validate_record(raw, root):
    if len(raw) > 2048 or not raw.endswith(b"\n") or raw.count(b"\n") != 1:
        raise ValueError("Probe output must be one bounded record with a final newline.")
    record = json.loads(raw, object_pairs_hook=unique_object)
    if type(record) is not dict or set(record) != EXPECTED:
        raise ValueError("Probe output has an unknown field or a missing field.")
    canonical = (json.dumps(record, separators=(",", ":"), ensure_ascii=True) + "\n").encode()
    if raw != canonical:
        raise ValueError("Probe output is not the canonical compact record.")
    if record["schema_version"] != 5 or type(record["schema_version"]) is not int:
        raise ValueError("The probe schema is not supported.")
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
            raise ValueError(f"The value for {key} is not an unsigned integer.")
    if not (record["docker_root_available_bytes"] <= record["docker_root_free_bytes"] <= record["docker_root_total_bytes"]):
        raise ValueError("Docker-root free bytes are greater than total bytes.")
    if record["docker_root_total_bytes"] == 0:
        raise ValueError("Docker-root total bytes must be greater than zero.")
    if record["probe_cgroup_cpu_period_micros"] == 0:
        raise ValueError("The probe cgroup CPU period must be greater than zero.")
    if record["docker_root_inode_free"] > record["docker_root_inode_total"] or record["docker_root_inode_available"] > record["docker_root_inode_free"]:
        raise ValueError("Docker-root free inodes are greater than total inodes.")
    policy = record["docker_root_inode_policy"]
    if policy not in ("finite", "dynamic_btrfs", "unknown"):
        raise ValueError("The inode policy is unknown.")
    inode_counts = (record["docker_root_inode_total"], record["docker_root_inode_free"], record["docker_root_inode_available"])
    filesystem_name = record["docker_root_fs_type_name"]
    if filesystem_name == "btrfs":
        if record["docker_root_fs_type_magic"] != 0x9123683E:
            raise ValueError("The Btrfs type name does not match its type value.")
        expected_policy = "dynamic_btrfs" if inode_counts == (0, 0, 0) else "unknown"
        if policy != expected_policy:
            raise ValueError("The Btrfs inode policy does not match its raw values.")
    elif (
        record["docker_root_device_major"] != record["docker_root_statx_device_major"]
        or record["docker_root_device_minor"] != record["docker_root_statx_device_minor"]
    ):
        raise ValueError("The mount and statx device identities differ outside Btrfs.")
    if filesystem_name != "btrfs" and policy == "dynamic_btrfs":
        raise ValueError("The dynamic inode policy requires Btrfs.")
    elif policy == "finite" and record["docker_root_inode_total"] == 0:
        raise ValueError("The finite inode policy has no inode total.")
    elif policy == "unknown" and record["docker_root_inode_total"] != 0:
        raise ValueError("The unknown inode policy has a finite inode total.")
    for key in ("probe_cgroup_memory_max_bytes", "probe_cgroup_swap_max_bytes", "probe_cgroup_cpu_quota_micros", "probe_cgroup_pids_max"):
        if record[key] is not None and not unsigned(record[key]):
            raise ValueError(f"The value for {key} must be null or an unsigned integer.")
    if record["probe_cgroup_memory_max_bytes"] is not None and record["probe_cgroup_memory_current_bytes"] > record["probe_cgroup_memory_max_bytes"]:
        raise ValueError("Probe cgroup memory use is greater than its limit.")
    if record["probe_cgroup_swap_max_bytes"] is not None and record["probe_cgroup_swap_current_bytes"] > record["probe_cgroup_swap_max_bytes"]:
        raise ValueError("Probe cgroup swap use is greater than its limit.")
    if record["probe_cgroup_cpu_quota_micros"] == 0:
        raise ValueError("The probe cgroup CPU quota must be greater than zero.")
    if record["probe_cgroup_pids_max"] == 0:
        raise ValueError("The probe cgroup process limit must be greater than zero.")
    leaf_cpus = record["probe_cgroup_cpuset_cpus_count"]
    if type(leaf_cpus) is not int or not 1 <= leaf_cpus <= MAX_EFFECTIVE_CPUS:
        raise ValueError("The probe CPU set count is outside its documented range.")
    ancestors = record["cpu_ancestor_limits"]
    if type(ancestors) is not list or len(ancestors) > MAX_CGROUP_ANCESTORS:
        raise ValueError("The CPU ancestor list is too long.")
    path_parts = cgroup_path(record["probe_cgroup_path"])
    if path_parts is None:
        raise ValueError("The probe cgroup path is malformed.")
    expected_paths = expected_ancestor_paths(path_parts)
    if len(ancestors) != len(expected_paths):
        raise ValueError("The CPU ancestor list does not include the cgroup path.")
    ancestor_fields = {"path", "cpuset_cpus_count", "cpu_quota_micros", "cpu_period_micros"}
    parent_cpu_count = None
    for ancestor, expected_path in zip(ancestors, expected_paths, strict=True):
        if type(ancestor) is not dict or set(ancestor) != ancestor_fields:
            raise ValueError("A CPU ancestor has an unknown or missing field.")
        if ancestor["path"] != expected_path:
            raise ValueError("The CPU ancestors are not ordered from root to parent.")
        cpu_count = ancestor["cpuset_cpus_count"]
        if type(cpu_count) is not int or not 1 <= cpu_count <= MAX_EFFECTIVE_CPUS:
            raise ValueError("An ancestor CPU set count is outside its documented range.")
        if parent_cpu_count is not None and cpu_count > parent_cpu_count:
            raise ValueError("An ancestor CPU set count is greater than its parent.")
        quota = ancestor["cpu_quota_micros"]
        if quota is not None and (not unsigned(quota) or quota == 0):
            raise ValueError("An ancestor CPU quota must be null or a positive integer.")
        if not unsigned(ancestor["cpu_period_micros"]) or ancestor["cpu_period_micros"] == 0:
            raise ValueError("An ancestor CPU period must be a positive integer.")
        parent_cpu_count = cpu_count
    if parent_cpu_count is not None and leaf_cpus > parent_cpu_count:
        raise ValueError("The probe CPU set count is greater than its parent.")
    host_pid = record["probe_host_pid"]
    namespace_pid = record["probe_namespace_pid"]
    if type(host_pid) is not int or host_pid <= 0 or type(namespace_pid) is not int or namespace_pid <= 0:
        raise ValueError("Probe process IDs must be positive integers.")
    if host_pid != namespace_pid:
        raise ValueError("The probe host and namespace process IDs differ.")
    if record["docker_root_mount_target"] != DOCKER_ROOT_TARGET:
        raise ValueError("The Docker-root target is not the fixed probe path.")
    if not valid_mount_source(record["docker_root_mount_source"]):
        raise ValueError("The Docker-root mount source is malformed.")
    if not valid_mount_root(record["docker_root_mount_root"]):
        raise ValueError("The Docker-root mount root is malformed.")
    if type(record["docker_root_fs_type_name"]) is not str or not record["docker_root_fs_type_name"]:
        raise ValueError("The filesystem type name is empty.")
    if type(record["guest_boot_id"]) is not str or len(record["guest_boot_id"]) != 36:
        raise ValueError("The guest boot ID is malformed.")
    psi = record["memory_psi_some_avg10_bps"]
    if psi is not None and (not unsigned(psi) or psi > 10_000):
        raise ValueError("The PSI value is outside its documented range.")
    if not root.is_dir():
        raise ValueError("The smoke mount source is not a directory.")


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: validate_smoke.py OUTPUT ROOT")
    raw = pathlib.Path(sys.argv[1]).read_bytes()
    validate_record(raw, pathlib.Path(sys.argv[2]))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"resource-probe smoke validation failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error

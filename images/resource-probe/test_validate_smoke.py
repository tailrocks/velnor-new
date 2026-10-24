"""Focused tests for the schema-5 resource-probe image smoke parser."""

import copy
import json
import pathlib
import tempfile
import unittest

import validate_smoke


def valid_record():
    return {
        "schema_version": 5,
        "docker_root_mount_target": "/velnor/docker-root",
        "docker_root_mount_source": "/dev/vdb",
        "docker_root_mount_root": "/_data",
        "docker_root_mount_id": 42,
        "docker_root_device_major": 0,
        "docker_root_device_minor": 52,
        "docker_root_statx_device_major": 0,
        "docker_root_statx_device_minor": 52,
        "docker_root_fs_type_magic": 0xEF53,
        "docker_root_fs_type_name": "ext4",
        "docker_root_filesystem_id": 99,
        "guest_boot_id": "01234567-89ab-cdef-0123-456789abcdef",
        "docker_root_free_bytes": 800,
        "docker_root_total_bytes": 1000,
        "docker_root_available_bytes": 700,
        "docker_root_inode_total": 100,
        "docker_root_inode_free": 90,
        "docker_root_inode_available": 80,
        "docker_root_inode_policy": "finite",
        "memory_available_bytes": 4000,
        "load_milli": 125,
        "memory_psi_some_avg10_bps": None,
        "probe_cgroup_path": "/docker/job/runner",
        "probe_cgroup_memory_max_bytes": 2000,
        "probe_cgroup_memory_current_bytes": 1000,
        "probe_cgroup_swap_max_bytes": None,
        "probe_cgroup_swap_current_bytes": 0,
        "probe_cgroup_cpu_quota_micros": 10000,
        "probe_cgroup_cpu_period_micros": 100000,
        "probe_cgroup_cpuset_cpus_count": 2,
        "cpu_ancestor_limits": [
            {
                "path": "/",
                "cpuset_cpus_count": 8,
                "cpu_quota_micros": None,
                "cpu_period_micros": 100000,
            },
            {
                "path": "/docker",
                "cpuset_cpus_count": 4,
                "cpu_quota_micros": 250000,
                "cpu_period_micros": 100000,
            },
            {
                "path": "/docker/job",
                "cpuset_cpus_count": 2,
                "cpu_quota_micros": None,
                "cpu_period_micros": 100000,
            },
        ],
        "probe_host_pid": 17,
        "probe_namespace_pid": 17,
        "probe_cgroup_pids_max": 128,
    }


class SmokeRecordTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.directory.name) / "docker-root"
        self.root.mkdir()

    def tearDown(self):
        self.directory.cleanup()

    def validate(self, record):
        raw = (json.dumps(record, separators=(",", ":"), ensure_ascii=True) + "\n").encode()
        validate_smoke.validate_record(raw, self.root)

    def assert_rejected(self, record):
        with self.assertRaises(ValueError):
            self.validate(record)

    def test_accepts_schema_five_with_mount_identity_and_ordered_ancestor_chain(self):
        self.validate(valid_record())

    def test_rejects_unsupported_schema_versions_and_boolean(self):
        for version in (0, 2, 3, 4, 6, True):
            with self.subTest(version=version):
                record = valid_record()
                record["schema_version"] = version
                self.assert_rejected(record)

    def test_rejects_missing_or_extra_top_level_fields(self):
        record = valid_record()
        del record["probe_host_pid"]
        self.assert_rejected(record)
        record = valid_record()
        record["unexpected"] = 1
        self.assert_rejected(record)

    def test_rejects_missing_or_malformed_mount_identity(self):
        record = valid_record()
        del record["docker_root_mount_source"]
        self.assert_rejected(record)
        record = valid_record()
        del record["docker_root_mount_root"]
        self.assert_rejected(record)
        for key, value in (
            ("docker_root_mount_source", None),
            ("docker_root_mount_source", 17),
            ("docker_root_mount_source", ""),
            ("docker_root_mount_source", "dev\nvolume"),
            ("docker_root_mount_source", "é" * 513),
            ("docker_root_mount_root", None),
            ("docker_root_mount_root", 17),
            ("docker_root_mount_root", ""),
            ("docker_root_mount_root", "relative"),
            ("docker_root_mount_root", "/a//b"),
            ("docker_root_mount_root", "/a/./b"),
            ("docker_root_mount_root", "/a/../b"),
            ("docker_root_mount_root", "/bad\\x"),
            ("docker_root_mount_root", "/bad\tb"),
            ("docker_root_mount_root", "/" + "a" * 1024),
        ):
            with self.subTest(key=key, value=value):
                record = valid_record()
                record[key] = value
                self.assert_rejected(record)

    def test_accepts_mount_root_slash_and_octal_escape(self):
        record = valid_record()
        record["docker_root_mount_source"] = "tmpfs"
        record["docker_root_mount_root"] = "/docker\\040root"
        self.validate(record)
        record["docker_root_mount_root"] = "/"
        self.validate(record)

    def test_rejects_nonfixed_mount_target(self):
        record = valid_record()
        record["docker_root_mount_target"] = "/other/root"
        self.assert_rejected(record)

    def test_rejects_cpu_ancestor_order_and_count_inconsistency(self):
        record = valid_record()
        record["cpu_ancestor_limits"].reverse()
        self.assert_rejected(record)
        record = valid_record()
        record["cpu_ancestor_limits"][1]["cpuset_cpus_count"] = 9
        self.assert_rejected(record)
        record = valid_record()
        record["probe_cgroup_cpuset_cpus_count"] = 0
        self.assert_rejected(record)

    def test_rejects_bad_cpu_limits_and_pid_identity(self):
        record = valid_record()
        record["cpu_ancestor_limits"][0]["cpu_quota_micros"] = 0
        self.assert_rejected(record)
        record = valid_record()
        record["cpu_ancestor_limits"][0]["cpu_period_micros"] = 0
        self.assert_rejected(record)
        record = valid_record()
        record["probe_namespace_pid"] = 1
        self.assert_rejected(record)

    def test_rejects_path_and_limit_inconsistency(self):
        record = valid_record()
        record["probe_cgroup_path"] = "/"
        self.assert_rejected(record)
        record = valid_record()
        record["probe_cgroup_memory_current_bytes"] = 2001
        self.assert_rejected(record)
        record = valid_record()
        record["probe_cgroup_pids_max"] = 0
        self.assert_rejected(record)

    def test_rejects_duplicate_fields_and_noncanonical_records(self):
        duplicate = b'{"schema_version":3,"schema_version":3}\n'
        with self.assertRaises(ValueError):
            validate_smoke.validate_record(duplicate, self.root)
        record = valid_record()
        raw = (json.dumps(copy.deepcopy(record), indent=2) + "\n").encode()
        with self.assertRaises(ValueError):
            validate_smoke.validate_record(raw, self.root)


if __name__ == "__main__":
    unittest.main()

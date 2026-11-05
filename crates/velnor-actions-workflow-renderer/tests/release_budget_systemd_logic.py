# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import sys
import unittest

spec = importlib.util.spec_from_file_location("hosted", sys.argv.pop(1))
hosted = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hosted)


class UnitOutcome(unittest.TestCase):
    def test_orphan_requires_exact_successful_main_and_stop_timeout(self):
        values = {"Result": "timeout", "ExecMainCode": "1", "ExecMainStatus": "0", "ActiveState": "failed", "LoadState": "loaded"}
        self.assertTrue(hosted.valid_orphan(1, 0, values))
        self.assertFalse(hosted.valid_orphan(0, 0, values))
        self.assertFalse(hosted.valid_orphan(1, 1, values))
        for key in values:
            changed = dict(values)
            changed[key] = "unexpected"
            self.assertFalse(hosted.valid_orphan(1, 0, changed))

    def test_cleanup_rejects_surviving_pid_or_populated_cgroup(self):
        self.assertTrue(hosted.cleanup_complete(False, False))
        self.assertFalse(hosted.cleanup_complete(True, False))
        self.assertFalse(hosted.cleanup_complete(False, True))
        self.assertFalse(hosted.cleanup_complete(True, True))


if __name__ == "__main__":
    unittest.main()

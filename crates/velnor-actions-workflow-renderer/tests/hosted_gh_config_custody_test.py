# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import io
import json
import os
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).with_name("hosted_gh_config_custody.py")
NAME = "velnor-actions-workflow-renderer::verified_gh_shell_public$procfs_config_custody_rejects_same_user_poisoning"
RECORDS = [
    {"type": "suite", "event": "started", "test_count": 1},
    {"type": "test", "event": "started", "name": NAME},
    {"type": "test", "event": "ok", "name": NAME},
    {"type": "suite", "event": "ok", "passed": 1, "failed": 0},
]


class Completed:
    def __init__(self, stdout, returncode=0):
        self.stdout = stdout
        self.returncode = returncode


class HostedCustodyTest(unittest.TestCase):
    def test_requires_exact_successful_named_test(self):
        valid = b"\n".join(json.dumps(item).encode() for item in RECORDS)
        wrong = valid.replace(NAME.encode(), b"another_test")
        duplicate = valid.replace(b'"passed": 1', b'"passed": 1, "passed": 1')
        for output, status, accepted in ((valid, 0, True), (valid, 1, False),
                                         (wrong, 0, False), (duplicate, 0, False),
                                         (b'{"type":"suite","event":"ok","passed":0}', 0, False)):
            with self.subTest(output=output, status=status), tempfile.TemporaryDirectory() as temp:
                cwd = os.getcwd()
                os.chdir(temp)
                try:
                    actual_open = open
                    def fixture_open(path, *args, **kwargs):
                        if path == "/etc/os-release":
                            return io.StringIO('ID=ubuntu\nVERSION_ID="24.04"\n')
                        return actual_open(path, *args, **kwargs)
                    with patch("builtins.open", side_effect=fixture_open), \
                            patch("subprocess.run", return_value=Completed(output, status)), \
                            patch.dict(os.environ, {"VELNOR_CHECK_HEAD": "a" * 40,
                                                   "VELNOR_CHECK_ID": "hosted-gh-config-custody",
                                                   "VELNOR_CHECK_PLATFORM": "linux_x64"}):
                        if accepted:
                            runpy.run_path(str(SCRIPT), run_name="__main__")
                        else:
                            with self.assertRaises(SystemExit):
                                runpy.run_path(str(SCRIPT), run_name="__main__")
                    self.assertEqual(Path("hosted-gh-config-evidence.json").exists(), accepted)
                finally:
                    os.chdir(cwd)


if __name__ == "__main__":
    unittest.main()

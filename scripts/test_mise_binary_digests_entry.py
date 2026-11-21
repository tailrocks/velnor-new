# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

DIGEST_TESTS = Path(__file__).resolve().parent / "test_mise_binary_digests.py"
SKIP_LINE = "minisign is not available on this platform; signature tests skipped; CI runs them"


class MissingMinisignEntryPoint(unittest.TestCase):
    def run_entry(self, env):
        bare = Path(tempfile.mkdtemp(prefix="mise-no-minisign."))
        self.addCleanup(shutil.rmtree, bare, ignore_errors=True)
        return subprocess.run(
            [sys.executable, str(DIGEST_TESTS)],
            capture_output=True,
            text=True,
            timeout=120,
            env={"PATH": str(bare), **env},
        )

    def test_absent_minisign_skips_with_one_line(self):
        result = self.run_entry({})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), [SKIP_LINE])

    def test_absent_minisign_fails_when_required(self):
        result = self.run_entry({"VELNOR_REQUIRE_MINISIGN": "1"})
        self.assertNotEqual(result.returncode, 0, result.stdout)


if __name__ == "__main__":
    unittest.main()

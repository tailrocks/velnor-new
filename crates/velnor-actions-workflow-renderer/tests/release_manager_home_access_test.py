# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_qualification_source import stage


class RunnerHomeTest(unittest.TestCase):
    def test_complete_qualification_runs_when_checkout_parent_is_private(self):
        source = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory(prefix="velnor-private-home-", dir="/run") as directory:
            root = Path(directory)
            os.chown(root, 65534, 65534)
            root.chmod(0o750)
            fixture = root / "renderer"
            fixture.mkdir()
            stage(source, fixture)
            result = subprocess.run(["/usr/bin/python3", "-I", "-S", "-B",
                                     str(fixture / "tests/release_manager_qualification.py"), "--execute"],
                                    env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "LANG": "C"},
                                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, timeout=240, check=False)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))

    def test_nested_manager_uses_staged_source_when_checkout_parent_is_private(self):
        source = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory(prefix="velnor-private-home-", dir="/run") as directory:
            root = Path(directory)
            os.chown(root, 65534, 65534)
            root.chmod(0o750)
            fixture = root / "renderer"
            fixture.mkdir()
            stage(source, fixture)
            test = fixture / "tests/release_manager_scenario_controller_test.py"
            command = ("import importlib.util,unittest;"
                       "spec=importlib.util.spec_from_file_location('fixture'," + repr(str(test)) + ");"
                       "module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);"
                       "unittest.main(module=module,defaultTest="
                       "'ScenarioControllerTest.test_qualification_capability_does_not_reach_publisher_service')")
            result = subprocess.run(["/usr/bin/python3", "-I", "-S", "-B", "-c", command],
                                    env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "LANG": "C"},
                                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, timeout=60, check=False)
            self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8", "replace"))


if __name__ == "__main__":
    required = {"test_complete_qualification_runs_when_checkout_parent_is_private",
                "test_nested_manager_uses_staged_source_when_checkout_parent_is_private"}
    if set(unittest.defaultTestLoader.getTestCaseNames(RunnerHomeTest)) != required:
        raise SystemExit("private home scenario inventory mismatch")
    result = unittest.TextTestRunner(verbosity=1).run(unittest.defaultTestLoader.loadTestsFromTestCase(RunnerHomeTest))
    if not result.wasSuccessful() or result.testsRun != len(required) or result.skipped or result.expectedFailures:
        raise SystemExit("private home qualification failed")

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class RunnerHomeTest(unittest.TestCase):
    def test_nested_manager_uses_staged_source_when_checkout_parent_is_private(self):
        source = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory(prefix="velnor-private-home-", dir="/run") as directory:
            root = Path(directory)
            os.chown(root, 65534, 65534)
            root.chmod(0o750)
            fixture = root / "renderer"
            for directory_name, names in (
                ("src", ("release_manager_bus.py", "release_manager_lifecycle.py", "release_supervisor_exec.py")),
                ("tests", ("release_manager_scenario_controller.py", "release_manager_scenario_controller_test.py")),
            ):
                (fixture / directory_name).mkdir(parents=True)
                for name in names:
                    shutil.copyfile(source / directory_name / name, fixture / directory_name / name)
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
    unittest.main()

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_qualification_source import FILES, stage


class QualificationSourceTest(unittest.TestCase):
    def test_staged_source_loads_complete_real_qualification(self):
        source = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            stage(source, destination)
            for subdirectory, names in FILES.items():
                self.assertEqual(set(path.name for path in (destination / subdirectory).iterdir()), set(names))
                for name in names:
                    path = destination / subdirectory / name
                    self.assertEqual(path.read_bytes(), (source / subdirectory / name).read_bytes())
                    self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o444)
                    self.assertEqual(path.stat().st_uid, os.geteuid())
            code = """import importlib.util, pathlib, sys
root = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(root / 'tests'))
sys.path.insert(0, str(root / 'src'))
import release_manager_qualification as qualification
import release_manager_scenario_controller_test
import release_watch_fixture
release_watch_fixture.load_watch(root / 'src')
for module_name, class_name, method in qualification.scenarios.values():
    spec = importlib.util.spec_from_file_location(module_name, root / 'tests' / (module_name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert callable(getattr(getattr(module, class_name), method))
"""
            result = subprocess.run([sys.executable, '-I', '-S', '-B', '-c', code, str(destination)],
                                    capture_output=True, text=True, timeout=30, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_staging_rejects_symlink_nonregular_and_oversized_source(self):
        source = Path(__file__).resolve().parents[1]
        for fault in ('symlink', 'directory', 'fifo', 'oversized'):
            with self.subTest(fault=fault), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                candidate = root / 'candidate'
                shutil.copytree(source / 'src', candidate / 'src')
                shutil.copytree(source / 'tests', candidate / 'tests')
                victim = candidate / 'src' / FILES['src'][0]
                victim.unlink()
                if fault == 'symlink':
                    victim.symlink_to(source / 'src' / FILES['src'][0])
                elif fault == 'directory':
                    victim.mkdir()
                elif fault == 'fifo':
                    os.mkfifo(victim)
                else:
                    victim.write_bytes(b'x' * (1024 * 1024 + 1))
                destination = root / 'staged'
                destination.mkdir()
                with self.assertRaises((OSError, SystemExit)):
                    stage(candidate, destination)


if __name__ == '__main__':
    unittest.main()

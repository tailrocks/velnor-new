"""Process-group containment tests; fixtures grant no candidate authority."""
import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ORCH_SOURCE = Path(__file__).resolve().parent


def _outer(source, cwd):
    return subprocess.run(
        [sys.executable, '-c', source], cwd=cwd, capture_output=True, text=True,
        start_new_session=True, timeout=10,
    )


class CandidateProcessTests(unittest.TestCase):
    def test_leader_captures_stdout_and_direct_status(self):
        source = f"""
import json, sys
sys.path.insert(0, {str(ORCH_SOURCE)!r})
from source_root_rust_candidate_process import observe_candidate_child
output, status, _wall = observe_candidate_child(
    ['/bin/sh', '-c', 'printf candidate; exit 0'],
    {{'PATH': '/usr/bin:/bin'}}, {str(ORCH_SOURCE)!r}, 4096, 5)
print(json.dumps({{'output': output.decode(), 'status': status}}))
"""
        with tempfile.TemporaryDirectory(prefix='root-candidate-process-') as directory:
            result = _outer(source, directory)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout), {'output': 'candidate', 'status': 0})

    def test_nonleader_rejects_before_popen(self):
        with tempfile.TemporaryDirectory(prefix='root-candidate-process-') as directory:
            marker = Path(directory) / 'spawned'
            inner = f"""
import sys
from pathlib import Path
sys.path.insert(0, {str(ORCH_SOURCE)!r})
import source_root_rust_candidate_process as process
from source_intent_cold_common import ColdSourceIntent
def forbidden(*_args, **_kwargs):
    Path({str(marker)!r}).write_text('spawned')
    raise AssertionError('popen reached')
process.subprocess.Popen = forbidden
try:
    process.observe_candidate_child(['/bin/echo', 'bad'], {{'PATH': '/usr/bin:/bin'}},
                                    {str(ORCH_SOURCE)!r}, 64, 1)
except ColdSourceIntent as error:
    print(str(error))
else:
    raise SystemExit(3)
"""
            outer = f"""
import json, subprocess, sys
inner = {inner!r}
result = subprocess.run([sys.executable, '-c', inner], capture_output=True, text=True)
print(json.dumps({{'returncode': result.returncode, 'stdout': result.stdout}}))
"""
            with tempfile.TemporaryDirectory(prefix='root-candidate-process-') as directory:
                result = _outer(outer, directory)
            self.assertEqual(result.returncode, 0, result.stderr)
            detail = json.loads(result.stdout)
            self.assertEqual(detail['returncode'], 0)
            self.assertIn('root_candidate_child_capsule', detail['stdout'])
            self.assertFalse(marker.exists())

    def test_timeout_kills_outer_capsule_and_descendant(self):
        with tempfile.TemporaryDirectory(prefix='root-candidate-process-') as directory:
            marker = Path(directory) / 'escaped'
            descendant = (
                'import pathlib,time; time.sleep(0.8); '
                f'pathlib.Path({str(marker)!r}).write_text("escaped")'
            )
            child = (
                'import subprocess,sys,time; '
                f'subprocess.Popen([sys.executable, "-c", {descendant!r}]); time.sleep(10)'
            )
            source = f"""
import sys
sys.path.insert(0, {str(ORCH_SOURCE)!r})
from source_root_rust_candidate_process import observe_candidate_child
observe_candidate_child([sys.executable, '-c', {child!r}],
                         {{'PATH': '/usr/bin:/bin'}}, {str(ORCH_SOURCE)!r}, 4096, 0.2)
"""
            result = _outer(source, directory)
            self.assertEqual(result.returncode, -signal.SIGKILL, result.stderr)
            import time
            time.sleep(1.0)
            self.assertFalse(marker.exists())

    def test_output_overflow_kills_outer_capsule(self):
        child = 'import sys,time; print("x" * 100000, flush=True); time.sleep(10)'
        source = f"""
import sys
sys.path.insert(0, {str(ORCH_SOURCE)!r})
from source_root_rust_candidate_process import observe_candidate_child
observe_candidate_child([sys.executable, '-c', {child!r}],
                         {{'PATH': '/usr/bin:/bin'}}, {str(ORCH_SOURCE)!r}, 1024, 5)
"""
        with tempfile.TemporaryDirectory(prefix='root-candidate-process-') as directory:
            result = _outer(source, directory)
        self.assertEqual(result.returncode, -signal.SIGKILL, result.stderr)

    def test_nonzero_exit_kills_forked_descendant(self):
        with tempfile.TemporaryDirectory(prefix='root-candidate-process-') as directory:
            marker = Path(directory) / 'escaped'
            descendant = (
                'import pathlib,time; time.sleep(0.8); '
                f'pathlib.Path({str(marker)!r}).write_text("escaped")'
            )
            child = (
                'import subprocess,sys; '
                f'subprocess.Popen([sys.executable, "-c", {descendant!r}], '
                'stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); raise SystemExit(7)'
            )
            source = f"""
import sys
sys.path.insert(0, {str(ORCH_SOURCE)!r})
from source_root_rust_candidate_process import observe_candidate_child
observe_candidate_child([sys.executable, '-c', {child!r}],
                         {{'PATH': '/usr/bin:/bin'}}, {str(ORCH_SOURCE)!r}, 4096, 5)
"""
            result = _outer(source, directory)
            self.assertEqual(result.returncode, -signal.SIGKILL, result.stderr)
            import time
            time.sleep(1.0)
            self.assertFalse(marker.exists())

    def test_stale_executable_fd_kills_capsule_without_running_artifact(self):
        with tempfile.TemporaryDirectory(prefix='root-candidate-process-') as directory:
            marker = Path(directory) / 'executed'
            artifact = (
                'import pathlib; pathlib.Path(' + repr(str(marker)) + ').write_text("executed")'
            )
            source = f"""
import os, sys
sys.path.insert(0, {str(ORCH_SOURCE)!r})
from source_root_rust_candidate_process import observe_candidate_child
descriptor = os.open('/dev/null', os.O_RDONLY)
os.close(descriptor)
observe_candidate_child([sys.executable, '-c', {artifact!r}],
                         {{'PATH': '/usr/bin:/bin'}}, {str(ORCH_SOURCE)!r}, 4096, 5,
                         executable_descriptor=descriptor)
"""
            result = _outer(source, directory)
            self.assertEqual(result.returncode, -signal.SIGKILL, result.stderr)
            self.assertFalse(marker.exists())


if __name__ == '__main__':
    unittest.main()

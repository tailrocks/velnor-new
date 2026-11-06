# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import base64
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
import unittest
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans


source_root = Path(__file__).resolve().parents[1] / 'src'
source = (source_root / 'release_publish_entry.py').read_text()
for marker, name in (('__VELNOR_MANAGER_PAYLOAD__', 'release_manager_bus.py'),
                     ('__VELNOR_WATCH_ADMISSION_PAYLOAD__', 'release_watch_admission.py'),
                     ('__VELNOR_LIFECYCLE_PAYLOAD__', 'release_manager_lifecycle.py'),
                     ('__VELNOR_HELPER_PAYLOAD__', 'release_supervisor_exec.py'),
                     ('__VELNOR_BOOTSTRAP_PAYLOAD__', 'release_generator_bootstrap.sh'),
                     ('__VELNOR_WATCH_BOOTSTRAP_PAYLOAD__', 'release_bootstrap_watch.py'),
                     ('__VELNOR_WATCH_PROGRAM_PAYLOAD__', 'release_launcher_watch.py'),
                     ('__VELNOR_WATCH_CLAIM_PAYLOAD__', 'release_supervisor_claim.py')):
    source = source.replace(marker, base64.b64encode((source_root / name).read_bytes()).decode())
source = source.replace('__VELNOR_JOB_TIMEOUT_SECONDS__', '1800')
bundle = {key: base64.b64encode((source_root / name).read_bytes()).decode('ascii')
          for key, name in (('coordinator', 'release_coordinator_exec.py'),
                            ('retained', 'release_retained_parent.py'),
                            ('source', 'release_source_executor.py'),
                            ('channel', 'release_source_channel.py'),
                            ('upload', 'release_upload_operation.py'),
                            ('broker', 'release_forge_broker.py'))}
marker = '__VELNOR_SUPERVISOR_PAYLOAD__'
if source.count(marker) != 1:
    raise ValueError('invalid supervisor payload marker')
source = source.replace(marker, base64.b64encode(json.dumps(bundle).encode('utf-8')).decode('ascii'))
entry = {'__name__': 'publisher_entry_fixture'}
exec(compile(source, 'release_publish_entry.py', 'exec'), entry)


class PublisherEntrySystemdTest(unittest.TestCase):
    def setUp(self):
        existing = snapshot_roots()
        self.addCleanup(cleanup_orphans, existing)
        self.watch = RootWatch(entry["module"]("test_bus", entry["MANAGER"]),
                               entry["module"]("test_lifecycle", entry["LIFECYCLE"]), source_root)
        self.addCleanup(self.watch.close)

    def test_root_bootstrap_finishes_only_after_unit_quiesces(self):
        self.assertEqual(os.geteuid(), 0)
        payload = json.loads(base64.b64decode(entry['SUPERVISOR'], validate=True))
        names = {'coordinator': 'release_coordinator_exec.py',
                 'retained': 'release_retained_parent.py',
                 'source': 'release_source_executor.py',
                 'channel': 'release_source_channel.py',
                 'upload': 'release_upload_operation.py',
                 'broker': 'release_forge_broker.py'}
        self.assertEqual(set(payload), set(names))
        for key, name in names.items():
            self.assertEqual(base64.b64decode(payload[key], validate=True), (source_root / name).read_bytes())
        self.assertNotIn('__VELNOR_', source)
        with tempfile.TemporaryDirectory(prefix='velnor-entry-test-', dir='/run') as root:
            path = Path(root)
            ready = path / 'ready'
            token = 'fixture-not-a-real-credential'
            script = 'test -n "$GH_TOKEN" && printf complete > "$HOME/ready"'
            deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 60_000_000_000
            entry['acquire'](root, script, {'PATH': '/usr/bin:/bin', 'HOME': root, 'GH_TOKEN': token}, deadline, self.watch.watch)
            self.assertEqual(ready.read_text(), 'complete')
            for file in path.rglob('*'):
                if file.is_file():
                    self.assertNotIn(token.encode(), file.read_bytes())

    def test_root_bootstrap_kills_detached_descendant_before_return(self):
        self.assertEqual(os.geteuid(), 0)
        with tempfile.TemporaryDirectory(prefix='velnor-entry-test-', dir='/run') as root:
            pidfile = Path(root) / 'pid'
            code = ('import os,signal,time\nr,w=os.pipe()\np=os.fork()\n'
                    'if p:\n os.close(w); os.read(r,1); os._exit(0)\n'
                    'os.close(r); os.setsid(); signal.signal(signal.SIGTERM,signal.SIG_IGN)\n'
                    f'open({str(pidfile)!r},"w").write(str(os.getpid()))\n'
                    'os.write(w,b"1"); time.sleep(300)')
            script = '/usr/bin/python3 -I -S -c ' + __import__('shlex').quote(code)
            deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 75_000_000_000
            try:
                entry['acquire'](root, script, {'PATH': '/usr/bin:/bin', 'HOME': root}, deadline, self.watch.watch)
            except Exception:
                pass
            self.assertTrue(pidfile.is_file())
            pid = int(pidfile.read_text())
            self.assertFalse(Path('/proc', str(pid)).exists())

    def test_root_bootstrap_manager_reaps_child_after_parent_death(self):
        self.assertEqual(os.geteuid(), 0)
        with tempfile.TemporaryDirectory(prefix='velnor-entry-death-', dir='/run') as root:
            path = Path(root)
            channel = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            channel.bind(str(path / 'sentinel'))
            channel.listen(1)
            channel.settimeout(10)
            ident = secrets.token_hex(16)
            unit = 'velnor-release-' + ident + '.service'
            directory = Path('/run/systemd/system') / (unit + '.d')
            command = ('import os,socket,sys,time;s=socket.socket(socket.AF_UNIX);'
                       's.connect(sys.argv[1]);s.send(str(os.getpid()).encode());s.close();time.sleep(300)')
            wrapper = (
                'import json,os,struct,sys,time;sys.path.insert(0,sys.argv[1]);'
                'import release_manager_bus as b,release_manager_lifecycle as l;'
                'from pathlib import Path;sys.path.insert(0,str(Path(sys.argv[1]).parent/"tests"));'
                'from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans;watch=RootWatch(b,l,Path(sys.argv[1]));'
                'l.secrets.token_hex=lambda _:sys.argv[3];'
                'm=b.Manager(sys.argv[2],2,watch.authority,root_bootstrap=True);'
                'p=json.dumps({"program":"/usr/bin/python3","args":["-I","-S","-c",sys.argv[4],sys.argv[5]],'
                '"cwd":"/","env":{},"stdin":""}).encode();'
                'd=time.clock_gettime_ns(time.CLOCK_BOOTTIME)+45_000_000_000;'
                'l.supervise(m,struct.pack(">QQI",d,os.stat("/proc/self/ns/time").st_ino,len(p))+p,d)'
            )
            parent = subprocess.Popen(['/usr/bin/python3', '-I', '-S', '-c', wrapper,
                                       str(source_root), str(source_root / 'release_supervisor_exec.py'),
                                       ident, command, str(path / 'sentinel')],
                                      stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                      stderr=subprocess.DEVNULL, env={'PATH': '/usr/bin:/bin'})
            try:
                connection, _ = channel.accept()
                with connection:
                    pid = int(connection.recv(32))
                parent.kill()
                parent.wait(timeout=5)
                end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 12_000_000_000
                while Path('/proc', str(pid)).exists() and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end:
                    time.sleep(0.05)
                self.assertFalse(Path('/proc', str(pid)).exists())
            finally:
                channel.close()
                if parent.poll() is None:
                    parent.kill()
                    parent.wait(timeout=5)
                subprocess.run(['/usr/bin/systemctl', 'stop', unit], stdout=subprocess.DEVNULL,
                               stderr=subprocess.DEVNULL, check=False, timeout=10)
                shutil.rmtree(directory, ignore_errors=True)


if __name__ == '__main__':
    unittest.main()

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import fcntl
import importlib.util
import json
import os
from pathlib import Path
import secrets
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans

source = Path(__file__).resolve().parents[1] / 'src'


def load(name):
    spec = importlib.util.spec_from_file_location(name, source / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


bus = load('release_manager_bus')
lifecycle = load('release_manager_lifecycle')
watch_module = load('release_bootstrap_watch')


def deadline(seconds=30):
    return time.clock_gettime_ns(time.CLOCK_BOOTTIME) + seconds * 1_000_000_000


class BootstrapWatchTest(unittest.TestCase):
    def test_root_death_drains_detached_workers_before_and_after_exec(self):
        for handoff in (False, True):
            with self.subTest(handoff=handoff):
                self.assert_root_death_drains(handoff)

    def assert_root_death_drains(self, handoff):
        before = snapshot_roots()
        process = subprocess.Popen(['/usr/bin/python3', '-I', '-S', str(Path(__file__).resolve()),
                                    '--watch-worker', str(int(handoff))],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, close_fds=True)
        identity = None
        try:
            self.assertTrue(select.select([process.stdout], [], [], 20)[0], 'worker never became ready')
            line = process.stdout.readline()
            if not line:
                self.fail('worker rejected: ' + process.stderr.read(4096))
            identity = json.loads(line)
            process.kill()
            process.wait(timeout=5)
            until = deadline()
            while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < until:
                alive = any(Path('/proc', str(pid)).exists() for pid in identity['pids'])
                group = Path('/sys/fs/cgroup/system.slice') / identity['unit']
                if not alive and not group.exists():
                    break
                time.sleep(.05)
            else:
                self.fail('detached worker survived root death')
            shutil.rmtree(Path('/run/systemd/system') / (identity['unit'] + '.d'))
            shutil.rmtree(identity['root'])
        finally:
            try:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
                process.communicate(timeout=5)
            finally:
                process.stdout.close()
                process.stderr.close()
                cleanup_orphans(before)


    def test_handoff_preserves_identity_and_helpers_cannot_inherit_writer(self):
        with RootWatch(bus, lifecycle, source) as fixture:
            watch = fixture.watch
            lease, writer, namespace, _operation = watch.authority
            child = subprocess.run(['/usr/bin/python3', '-I', '-S', '-c',
                                    'import os,sys; p=sys.argv[1]; '
                                    'assert all(os.readlink("/proc/self/fd/"+n)!=p '
                                    'for n in os.listdir("/proc/self/fd") if n not in ("0","1","2","3"))',
                                    os.readlink('/proc/self/fd/' + str(writer))],
                                   close_fds=True, check=False)
            self.assertEqual(child.returncode, 0)
            env = watch.prepare_exec_environment()
            self.assertTrue(os.get_inheritable(writer))
            self.assertTrue(os.get_inheritable(lease))
            previous = {key: os.environ.get(key) for key in env}
            os.environ.update(env)
            try:
                recovered = watch_module.reclaim(bus, lifecycle, fixture.deadline, 'publish',
                    (source / 'release_launcher_watch.py').read_bytes(),
                    (source / 'release_supervisor_claim.py').read_bytes())
                self.assertEqual(recovered.data, watch.data)
                self.assertEqual(recovered.authority, (lease, writer, namespace, 'publish'))
                recovered.manager.close()
            finally:
                for key, value in previous.items():
                    if value is None:
                        os.environ.pop(key, None)
                    else:
                        os.environ[key] = value
            self.assertFalse(os.get_inheritable(writer))
            self.assertFalse(os.get_inheritable(lease))

    def test_forged_lease_and_wrong_operation_are_rejected_before_unit_creation(self):
        with RootWatch(bus, lifecycle, source) as fixture:
            lease, writer, namespace, operation = fixture.authority
            original = fixture.watch.data
            for field, value in [('invocation', 'a' * 32), ('deadline', 1),
                                 ('namespace', 'b' * 32), ('time_namespace', original['time_namespace'] + 1),
                                 ('staging_inode', original['staging_inode'] + 1)]:
                with self.subTest(field=field):
                    changed = dict(original, **{field: value})
                    fd = os.memfd_create('invalid-watch', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
                    try:
                        os.fchmod(fd, 0o600)
                        os.write(fd, json.dumps(changed, sort_keys=True, separators=(',', ':')).encode())
                        fcntl.fcntl(fd, fcntl.F_ADD_SEALS,
                            fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
                        with self.assertRaises(Exception):
                            connection = bus.Manager(str(source / 'release_supervisor_exec.py'), 30,
                                                     (fd, writer, namespace, operation))
                            try:
                                connection.subscribe(deadline())
                                reader, sink = os.pipe()
                                try:
                                    connection.start('velnor-release-' + secrets.token_hex(16) + '.service',
                                                     reader, deadline())
                                finally:
                                    os.close(reader)
                                    os.close(sink)
                            finally:
                                connection.close()
                    finally:
                        os.close(fd)
            with self.assertRaises(bus.ManagerError):
                bus.read_watch((lease, writer, namespace, 'preflight'))


def watch_worker(handoff, saved=None):
    if saved is None:
        fixture = RootWatch(bus, lifecycle, source)
        watch = fixture.watch
        end = fixture.deadline
        if handoff:
            env = {'PATH': '/usr/bin:/bin', 'LANG': 'C'}
            env.update(watch.prepare_exec_environment())
            os.execve('/usr/bin/python3', ['/usr/bin/python3', '-I', '-S', str(Path(__file__).resolve()),
                      '--watch-reclaimed', str(end)], env)
    else:
        end = saved
        watch = watch_module.reclaim(bus, lifecycle, end, 'publish',
            (source / 'release_launcher_watch.py').read_bytes(),
            (source / 'release_supervisor_claim.py').read_bytes())
    root = Path(tempfile.mkdtemp(prefix='velnor-publisher-', dir='/run'))
    helper = root / 'helper.py'
    shutil.copyfile(source / 'release_supervisor_exec.py', helper)
    helper.chmod(0o500)
    marker = root / 'workers.json'
    pipe = os.fstat(watch.authority[1])
    code = ("import json,os,signal,sys,time\n"
            "for n in os.listdir('/proc/self/fd'):\n"
            " try: f=os.fstat(int(n))\n"
            " except OSError: continue\n"
            f" if (f.st_dev,f.st_ino)==({pipe.st_dev},{pipe.st_ino}): raise SystemExit(9)\n"
            "pid=os.fork()\n"
            "if pid == 0:\n os.setsid(); signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(300)\n"
            "else:\n open(sys.argv[1],'w').write(json.dumps([os.getpid(),pid])); time.sleep(300)\n")
    body = json.dumps({'program': '/usr/bin/python3', 'args': ['-I', '-S', '-c', code, str(marker)],
                       'cwd': '/', 'env': {'PATH': '/usr/bin:/bin'}, 'stdin': ''}).encode()
    manager = bus.Manager(str(helper), 300, watch.authority, root_bootstrap=True)
    manager.subscribe(end)
    unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
    reader, writer = os.pipe2(os.O_CLOEXEC)
    job = manager.start(unit, reader, end)
    os.close(reader)
    lifecycle.wait_job(manager, unit, job, end)
    lifecycle.write_frame(writer, struct.pack('>QQI', end, os.stat('/proc/self/ns/time').st_ino, len(body)) + body, end)
    os.close(writer)
    until = deadline()
    while not marker.exists() and time.clock_gettime_ns(time.CLOCK_BOOTTIME) < until:
        time.sleep(.01)
    print(json.dumps({'unit': unit, 'pids': json.loads(marker.read_text()), 'root': str(root)}), flush=True)
    time.sleep(300)


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--watch-worker':
        watch_worker(bool(int(sys.argv[2])))
    elif len(sys.argv) == 3 and sys.argv[1] == '--watch-reclaimed':
        watch_worker(False, int(sys.argv[2]))
    else:
        unittest.main()

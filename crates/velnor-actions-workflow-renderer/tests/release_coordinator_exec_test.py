# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import struct
import subprocess
import tempfile
import time
import unittest
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_watch_fixture import RootWatch, snapshot_roots, cleanup_orphans

root = Path(__file__).resolve().parents[1]

def load(name):
    spec = importlib.util.spec_from_file_location(name, root / 'src' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

bus = load('release_manager_bus')
lifecycle = load('release_manager_lifecycle')

class CoordinatorChannelTest(unittest.TestCase):
    def setUp(self):
        existing = snapshot_roots()
        self.addCleanup(cleanup_orphans, existing)
        self.watch = RootWatch(bus, lifecycle, root / "src")
        self.addCleanup(self.watch.close)

    def test_coordinator_rejects_publishing_credentials_in_bootstrap(self):
        self.assert_bootstrap_rejects(None)
        for key in ('CARGO_REGISTRY_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_URL'):
            with self.subTest(key=key):
                self.assert_bootstrap_rejects(key)

    def assert_bootstrap_rejects(self, key):
        with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-') as directory:
            directory = Path(directory)
            helper = directory / 'helper.py'
            shutil.copyfile(root / 'src/release_coordinator_exec.py', helper)
            helper.chmod(0o500)
            source = directory / 'fixture.c'
            marker = directory / 'executed'
            source.write_text('#include <fcntl.h>\n#include <unistd.h>\n#include <stdlib.h>\n#include <stdio.h>\n'
                              'int main(void) { char path[4096]; if(!getenv("HOME")) return 2; '
                              'snprintf(path,sizeof(path),"%s/executed",getenv("HOME")); '
                              'int fd=open(path,O_CREAT|O_WRONLY,0600); '
                              'if(fd<0) return 1; close(fd); return 0; }\n')
            bindir = directory / 'velnor/bin'
            bindir.mkdir(parents=True, mode=0o700)
            bindir.parent.chmod(0o700)
            binary = bindir / 'velnor-actions-0.1.74'
            prebuilt = os.environ.get('VELNOR_TEST_NATIVE_MARKER_BINARY')
            if prebuilt:
                shutil.copyfile(prebuilt, binary)
            else:
                subprocess.run(['/usr/bin/cc', str(source), '-o', str(binary)], check=True)
            binary.chmod(0o500)
            parent, child = socket.socketpair()
            manager = bus.Manager(str(helper), 10, self.watch.authority, root_bootstrap=True)
            unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
            end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 35_000_000_000
            try:
                owner = manager.subscribe(end)
                job = manager.start(unit, child.fileno(), end)
                child.close()
                lifecycle.wait_job(manager, unit, job, end)
                data = json.dumps({'version': '0.1.74', 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                                   'env': {} if key is None else {key: 'synthetic-canary'}}, separators=(',', ':')).encode()
                parent.sendall(struct.pack('>QQI', end, os.stat('/proc/self/ns/time').st_ino, len(data)) + data)
                while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end:
                    state = lifecycle.state(manager, owner, unit, end)
                    if state['active'] in ('inactive', 'failed'):
                        self.assertFalse(state['populated'])
                        self.assertIsNone(state['job'])
                        if key is None:
                            self.assertTrue(marker.exists(), 'valid coordinator did not execute')
                            self.assertEqual(state['result'], 'success')
                        else:
                            self.assertFalse(marker.exists(), 'coordinator executed with forbidden credential')
                            self.assertNotEqual(state['result'], 'success')
                        manager.release(unit, end)
                        break
                    time.sleep(0.01)
                else:
                    self.fail('coordinator did not finish')
            finally:
                parent.close()
                child.close()
                if manager.dropin_name:
                    subprocess.run(['/usr/bin/systemctl', 'stop', unit], check=False, timeout=10)
                    manager.release(unit, time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 10_000_000_000)
                manager.close()

    def test_root_coordinator_keeps_bidirectional_channel(self):
        with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-') as directory:
            directory = Path(directory)
            helper = directory / 'helper.py'
            shutil.copyfile(root / 'src/release_coordinator_exec.py', helper)
            helper.chmod(0o500)
            code = directory / 'fixture.c'
            code.write_text('#include <unistd.h>\n#include <string.h>\n#include <fcntl.h>\n#include <stdio.h>\n'
                            'int main(int n, char **a) { char b[128]; int caps=0, bound=0; '
                            'FILE *f=fopen("/proc/self/status","r"); if(!f) return 4; '
                            'while(fgets(b,sizeof(b),f)) { '
                            'if(!strcmp(b,"CapEff:\\t0000000000000000\\n")) caps=1; '
                            'if(!strcmp(b,"CapBnd:\\t0000000000000000\\n")) bound=1; } '
                            'fclose(f); if(!caps || !bound) return 5; '
                            'for(int i=3;i<1024;i++) if(fcntl(i,F_GETFD)!=-1) return 6; '
                            'if (geteuid()!=0 || n!=3 || strcmp(a[1],"crate-release") || '
                            'strcmp(a[2],"publish-supervise")) return 1; '
                            'if (read(0,b,4)!=4 || memcmp(b,"ping",4)) return 2; '
                            'return write(0,"pong",4)!=4; }\n')
            bindir = directory / 'velnor/bin'
            bindir.mkdir(parents=True)
            bindir.chmod(0o700)
            bindir.parent.chmod(0o700)
            binary = bindir / 'velnor-actions-0.1.74'
            subprocess.run(['/usr/bin/cc', str(code), '-o', str(binary)], check=True)
            binary.chmod(0o500)
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            parent, child = socket.socketpair()
            manager = bus.Manager(str(helper), 10, self.watch.authority, root_bootstrap=True)
            unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
            end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 35_000_000_000
            try:
                owner = manager.subscribe(end)
                job = manager.start(unit, child.fileno(), end)
                child.close()
                lifecycle.wait_job(manager, unit, job, end)
                data = json.dumps({'version': '0.1.74', 'sha256': digest, 'env': {}}, separators=(',', ':')).encode()
                parent.sendall(struct.pack('>QQI', end, os.stat('/proc/self/ns/time').st_ino, len(data)) + data)
                parent.sendall(b'ping')
                parent.settimeout(10)
                self.assertEqual(parent.recv(4), b'pong')
                while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end:
                    state = lifecycle.state(manager, owner, unit, end)
                    if state['active'] == 'inactive':
                        self.assertFalse(state['populated'])
                        self.assertIsNone(state['job'])
                        self.assertEqual(state['result'], 'success')
                        self.assertEqual(state['exec_status'], 0)
                        manager.release(unit, end)
                        break
                    self.assertNotEqual(state['active'], 'failed')
                    time.sleep(0.01)
                else:
                    self.fail('coordinator did not finish')
            finally:
                parent.close()
                child.close()
                if manager.dropin_name:
                    subprocess.run(['/usr/bin/systemctl', 'stop', unit], check=False, timeout=10)
                    manager.release(unit, time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 10_000_000_000)
                manager.close()

    def test_bound_unit_stops_when_supervisor_unit_dies(self):
        with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-') as directory:
            directory = Path(directory)
            helper = directory / 'source-helper.py'
            shutil.copyfile(root / 'src/release_supervisor_exec.py', helper)
            helper.chmod(0o500)
            parent = self.watch.unit
            unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
            manager = bus.Manager(str(helper), 10, self.watch.authority, root_bootstrap=True)
            reader, writer = os.pipe()
            end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 35_000_000_000
            try:
                owner = manager.subscribe(end)
                job = manager.start(unit, reader, end)
                os.close(reader)
                reader = None
                lifecycle.wait_job(manager, unit, job, end)
                current = lifecycle.state(manager, owner, unit, end)
                self.assertEqual(current['active'], 'active')
                subprocess.run(['/usr/bin/systemctl', 'kill', '--kill-whom=main', '-s', 'KILL', parent],
                               check=True, timeout=10)
                while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end:
                    current = lifecycle.state(manager, owner, unit, end)
                    if current['active'] in ('inactive', 'failed') and not current['populated'] and current['job'] is None:
                        break
                    time.sleep(0.01)
                else:
                    self.fail('bound child remained live after parent unit death')
                manager.release(unit, end)
            finally:
                if reader is not None:
                    os.close(reader)
                os.close(writer)
                subprocess.run(['/usr/bin/systemctl', 'stop', parent, unit], check=False,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
                if manager.dropin_name:
                    manager.release(unit, time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 10_000_000_000)
                manager.close()

    def test_preclaim_rejects_late_reactivation(self):
        late = 'velnor-release-' + secrets.token_hex(16) + '.service'
        claim = self.watch.directory / 'supervisor.claim'
        self.assertTrue(claim.is_file())
        manager = bus.Manager(str(root / 'src/release_supervisor_exec.py'), 10,
                              self.watch.authority, root_bootstrap=True)
        reader, writer = os.pipe()
        end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 35_000_000_000
        try:
            subprocess.run(['/usr/bin/systemctl', 'kill', '--kill-whom=main', '-s', 'KILL', self.watch.unit],
                           check=True, timeout=10)
            while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < end:
                state = self.watch.watch.manager.state(self.watch.unit, end)
                if state['active'] in ('failed', 'inactive') and not state['populated']:
                    break
                time.sleep(.01)
            else:
                self.fail('watch did not stop')
            manager.subscribe(end)
            with self.assertRaises(bus.ManagerError):
                manager.start(late, reader, end)
            self.assertFalse(Path('/run/systemd/system', late + '.d').exists())
            self.assertTrue(claim.is_file())
        finally:
            os.close(reader)
            os.close(writer)
            manager.close()

    def test_parent_death_stops_coordinator(self):
        with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-') as directory:
            directory = Path(directory)
            helper = directory / 'helper.py'
            shutil.copyfile(root / 'src/release_coordinator_exec.py', helper)
            helper.chmod(0o500)
            bindir = directory / 'velnor/bin'
            bindir.mkdir(parents=True)
            bindir.chmod(0o700)
            bindir.parent.chmod(0o700)
            code = directory / 'fixture.c'
            code.write_text('#include <unistd.h>\nint main(void) { '
                            'if(write(0,"ready",5)!=5) return 1; for(;;) pause(); }\n')
            binary = bindir / 'velnor-actions-0.1.74'
            subprocess.run(['/usr/bin/cc', str(code), '-o', str(binary)], check=True)
            binary.chmod(0o500)
            digest = hashlib.sha256(binary.read_bytes()).hexdigest()
            unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
            report_read, report_write = os.pipe()
            child_pid = os.fork()
            if child_pid == 0:
                try:
                    os.close(report_read)
                    os.close(self.watch.authority[0])
                    os.close(self.watch.authority[1])
                    child_watch = RootWatch(bus, lifecycle, root / "src")
                    parent, child = socket.socketpair()
                    manager = bus.Manager(str(helper), 10, child_watch.authority, root_bootstrap=True)
                    end = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 30_000_000_000
                    manager.subscribe(end)
                    job = manager.start(unit, child.fileno(), end)
                    child.close()
                    lifecycle.wait_job(manager, unit, job, end)
                    data = json.dumps({'version': '0.1.74', 'sha256': digest, 'env': {}}).encode()
                    parent.sendall(struct.pack('>QQI', end, os.stat('/proc/self/ns/time').st_ino, len(data)) + data)
                    parent.settimeout(10)
                    if parent.recv(5) != b'ready':
                        os._exit(2)
                    os.write(report_write, b'1')
                    os._exit(0)
                except BaseException:
                    os._exit(3)
            os.close(report_write)
            try:
                self.assertEqual(os.read(report_read, 1), b'1')
                _, status = os.waitpid(child_pid, 0)
                self.assertEqual(status, 0)
                time.sleep(0.5)
                active = subprocess.run(['/usr/bin/systemctl', 'is-active', unit], check=False,
                                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=10)
                self.assertNotEqual(active.stdout.strip(), b'active', 'coordinator survived parent death')
            finally:
                os.close(report_read)
                subprocess.run(['/usr/bin/systemctl', 'stop', unit], check=False,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
                shutil.rmtree('/run/systemd/system/' + unit + '.d', ignore_errors=True)

if __name__ == '__main__':
    unittest.main()

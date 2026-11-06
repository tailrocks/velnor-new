# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import importlib.util
import json
import os
import pathlib
import shutil
import socket
import subprocess
import struct
import tempfile
import time
import unittest

root = pathlib.Path(__file__).resolve().parents[1] / 'src'


def load(name):
    spec = importlib.util.spec_from_file_location(name, root / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RetainedEntryTest(unittest.TestCase):
    def test_source_unit_denies_network_when_requested(self):
        bus = load('release_manager_bus')
        lifecycle = load('release_manager_lifecycle')
        with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-') as directory, \
                socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            pathlib.Path(directory).chmod(0o711)
            helper = pathlib.Path(directory) / 'helper.py'
            shutil.copyfile(root / 'release_supervisor_exec.py', helper)
            helper.chmod(0o555)
            listener.bind(('127.0.0.1', 0))
            listener.listen(1)
            output = os.memfd_create('fixture-output', os.MFD_CLOEXEC)
            manager = bus.Manager(str(helper), 20, deny_network=True)
            deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 45_000_000_000
            code = ('import socket,sys; s=socket.socket(); s.settimeout(1); '
                    'assert s.connect_ex(("127.0.0.1",int(sys.argv[1]))) != 0; print("isolated")')
            payload = json.dumps({'program': '/usr/bin/python3',
                                  'args': ['-I', '-S', '-c', code, str(listener.getsockname()[1])],
                                  'cwd': '/', 'env': {}, 'stdin': ''}).encode()
            frame = struct.pack('>QQI', deadline, os.stat('/proc/self/ns/time').st_ino, len(payload)) + payload
            try:
                lifecycle.supervise(manager, frame, deadline, output_fd=output)
                os.lseek(output, 0, os.SEEK_SET)
                self.assertEqual(os.read(output, 128), b'isolated\n')
                self.assertIsNone(manager.dropin_name)
            finally:
                manager.close()
                os.close(output)

    def test_source_unit_uses_private_tmp_and_denies_unix_sockets(self):
        bus = load('release_manager_bus')
        lifecycle = load('release_manager_lifecycle')
        with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-') as directory, \
                tempfile.NamedTemporaryFile(dir='/tmp', prefix='velnor-sentinel-') as sentinel:
            helper = pathlib.Path(directory) / 'helper.py'
            pathlib.Path(directory).chmod(0o711)
            shutil.copyfile(root / 'release_supervisor_exec.py', helper)
            helper.chmod(0o555)
            output = os.memfd_create('fixture-output', os.MFD_CLOEXEC)
            manager = bus.Manager(str(helper), 20, deny_unix=True)
            deadline = time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 45_000_000_000
            code = ('import os,socket,sys; assert not os.path.exists(sys.argv[1]); '
                    'failed=False; '
                    '\ntry: socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)'
                    '\nexcept OSError: failed=True'
                    '\nassert failed; print("isolated")')
            payload = json.dumps({'program': '/usr/bin/python3', 'args': ['-I', '-S', '-c', code, sentinel.name],
                                  'cwd': '/', 'env': {}, 'stdin': ''}).encode()
            frame = struct.pack('>QQI', deadline, os.stat('/proc/self/ns/time').st_ino, len(payload)) + payload
            try:
                lifecycle.supervise(manager, frame, deadline, output_fd=output)
                os.lseek(output, 0, os.SEEK_SET)
                self.assertEqual(os.read(output, 128), b'isolated\n')
                self.assertIsNone(manager.dropin_name)
            finally:
                manager.close()
                os.close(output)

    def test_parent_strips_publish_credentials_and_reaps_units(self):
        bus = load('release_manager_bus')
        lifecycle = load('release_manager_lifecycle')
        retained = load('release_retained_parent')
        channel = load('release_source_channel')
        with tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-private-') as private_dir, \
                tempfile.TemporaryDirectory(dir='/run', prefix='velnor-publisher-view-') as view_dir:
            private = pathlib.Path(private_dir)
            view = pathlib.Path(view_dir)
            for name in ('release_launcher_watch.py', 'release_supervisor_claim.py', 'release_coordinator_exec.py'):
                shutil.copyfile(root / name, private / name)
                (private / name).chmod(0o500)
            helper = view / 'release_supervisor_exec.py'
            shutil.copyfile(root / 'release_supervisor_exec.py', helper)
            helper.chmod(0o555)
            target = private / 'velnor/bin'
            target.mkdir(parents=True, mode=0o700)
            target.parent.chmod(0o700)
            source = private / 'fixture.c'
            source.write_text('#include <fcntl.h>\n#include <unistd.h>\n#include <stdlib.h>\n#include <stdio.h>\n'
                'int main(void) { char p[4096]; if(getenv("CARGO_REGISTRY_TOKEN") || '
                'getenv("ACTIONS_ID_TOKEN_REQUEST_TOKEN") || getenv("ACTIONS_ID_TOKEN_REQUEST_URL") || '
                '!getenv("HOME")) return 2; snprintf(p,sizeof(p),"%s/clean-env",getenv("HOME")); '
                'int fd=open(p,O_CREAT|O_WRONLY,0600); if(fd<0) return 1; close(fd); return 0; }\n')
            binary = target / 'velnor-actions-0.1.74'
            subprocess.run(['/usr/bin/cc', str(source), '-o', str(binary)], check=True)
            binary.chmod(0o500)
            before = {p.name for p in pathlib.Path('/sys/fs/cgroup/system.slice').glob('velnor-release-*.service')}
            env = {'CARGO_REGISTRY_TOKEN': 'synthetic-canary', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'synthetic-canary',
                   'ACTIONS_ID_TOKEN_REQUEST_URL': 'synthetic-canary'}
            with self.assertRaises(Exception):
                retained.run(str(private), str(view), hashlib.sha256(helper.read_bytes()).hexdigest(),
                             '0.1.74', hashlib.sha256(binary.read_bytes()).hexdigest(), env,
                             time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 80_000_000_000,
                             bus, lifecycle, channel, None, None, None)
            self.assertTrue((private / 'clean-env').is_file(), 'coordinator did not run with a clean environment')
            after = {p.name for p in pathlib.Path('/sys/fs/cgroup/system.slice').glob('velnor-release-*.service')}
            self.assertFalse(after - before, 'retained parent left an active service cgroup')

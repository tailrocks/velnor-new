# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
from pathlib import Path
import secrets
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest

SOURCE = Path(__file__).resolve().parents[1] / 'src'
sys.path.insert(0, str(SOURCE))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_manager_bus as bus
import release_manager_lifecycle as lifecycle
from release_watch_fixture import RootWatch


def deadline():
    return time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 30_000_000_000


def environment_keys(pid):
    descriptor = os.open('/proc/' + str(pid) + '/environ', os.O_RDONLY | os.O_CLOEXEC)
    try:
        raw = bytearray()
        while chunk := os.read(descriptor, 4096):
            raw.extend(chunk)
            if len(raw) > 65536:
                raise ValueError('unit environment bound')
    finally:
        os.close(descriptor)
    return {item.partition(b'=')[0].decode('ascii') for item in raw.split(b'\0') if item}


class CleanProfileTest(unittest.TestCase):
    def test_real_profile_start_sanitizes_environment_and_closes(self):
        with RootWatch(bus, lifecycle, SOURCE, seconds=30) as fixture:
            manager = fixture.watch.manager
            unit = fixture.unit
            path = manager._method(manager.owner, '/org/freedesktop/systemd1',
                'org.freedesktop.systemd1.Manager', 'GetUnit', [('s', unit)], fixture.deadline, 'o')
            pid = manager._get(path, 'org.freedesktop.systemd1.Service', 'MainPID', 'u', fixture.deadline)
            self.assertGreater(pid, 0)
            self.assertFalse(set(manager.policy_host.environment_names) & environment_keys(pid))
        self.assertFalse(Path('/sys/fs/cgroup/system.slice', unit).exists())


class ContainerPolicyTest(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix='velnor-publisher-', dir='/run'))
        self.root.chmod(0o700)
        self.addCleanup(shutil.rmtree, self.root)

    def test_inherited_prefix_policy_denies_before_any_payload(self):
        for index, scope in enumerate(('velnor-.service.d', 'velnor-release-.service.d', 'service.d')):
            with self.subTest(scope=scope):
                directory = Path('/run/systemd/system', scope)
                directory.mkdir(mode=0o700)
                marker = self.root / ('marker-' + str(index) + '-%n')
                before = set(Path('/run/systemd/system').glob('velnor-release-*.service.d'))
                try:
                    (directory / 'policy.conf').write_text('[Service]\nExecStartPre=/usr/bin/touch ' + str(marker) + '\n')
                    rejected = None
                    try:
                        with RootWatch(bus, lifecycle, SOURCE, seconds=30):
                            pass
                    except bus.StartRejected as error:
                        rejected = error
                    self.assertFalse(list(self.root.glob('marker-' + str(index) + '-velnor-release-*')),
                                     'inherited hook executed before admission')
                    self.assertIsNotNone(rejected, 'unsupported inherited policy was admitted')
                    self.assertIn('unreviewed applicable manager policy', str(rejected.__cause__))
                    self.assertEqual(set(Path('/run/systemd/system').glob('velnor-release-*.service.d')), before)
                    self.assertFalse(Path('/sys/fs/cgroup/system.slice', rejected.unit).exists())
                finally:
                    (directory / 'policy.conf').unlink()
                    directory.rmdir()

    def test_unknown_global_bytes_deny_without_unit(self):
        path = Path('/etc/systemd/system.conf')
        original = path.read_bytes()
        before = set(Path('/run/systemd/system').glob('velnor-release-*.service.d'))
        try:
            path.write_bytes(original + b'\n# unsupported fixture policy\n')
            with self.assertRaises(bus.StartRejected) as rejected:
                with RootWatch(bus, lifecycle, SOURCE, seconds=30):
                    pass
            self.assertIn('unreviewed manager policy bytes', str(rejected.exception.__cause__))
            self.assertEqual(set(Path('/run/systemd/system').glob('velnor-release-*.service.d')), before)
            self.assertFalse(Path('/sys/fs/cgroup/system.slice', rejected.exception.unit).exists())
        finally:
            path.write_bytes(original)

    def test_unknown_manager_environment_denies_before_staging(self):
        before = set(Path('/run/systemd/system').glob('velnor-release-*.service.d'))
        subprocess.run(['/usr/bin/systemctl', 'set-environment', 'VELNOR_POLICY_PROBE=public-test'], check=True)
        try:
            with self.assertRaises(bus.StartRejected) as rejected:
                with RootWatch(bus, lifecycle, SOURCE, seconds=30):
                    pass
            self.assertIn('unsupported authenticated manager policy', str(rejected.exception.__cause__))
            self.assertEqual(set(Path('/run/systemd/system').glob('velnor-release-*.service.d')), before)
            self.assertFalse(Path('/sys/fs/cgroup/system.slice', rejected.exception.unit).exists())
        finally:
            subprocess.run(['/usr/bin/systemctl', 'unset-environment', 'VELNOR_POLICY_PROBE'], check=True)

    def test_foreign_staging_is_unchanged_after_rejection(self):
        unit = 'velnor-release-' + secrets.token_hex(16) + '.service'
        directory = Path('/run/systemd/system', unit + '.d')
        directory.mkdir(mode=0o700)
        foreign = directory / 'foreign.conf'
        foreign.write_bytes(b'foreign-scratch')
        initial = foreign.stat()
        self.addCleanup(self.remove_foreign, unit, foreign, directory, initial)
        helper = self.root / 'release_launcher_watch.py'
        claim = self.root / 'release_supervisor_claim.py'
        for path in (helper, claim):
            path.write_bytes((SOURCE / path.name).read_bytes())
            path.chmod(0o500)
        manager = bus.Manager.watch_manager(str(helper), str(claim), 30, unit,
            hashlib.sha256(helper.read_bytes()).hexdigest(), hashlib.sha256(claim.read_bytes()).hexdigest())
        reader, writer = os.pipe2(os.O_CLOEXEC)
        try:
            end = deadline()
            manager.subscribe(end)
            os.write(writer, struct.pack('>QQ', end, os.stat('/proc/self/ns/time').st_ino))
            with self.assertRaises(bus.StartRejected):
                manager.start(unit, reader, end)
            self.assertEqual(foreign.read_bytes(), b'foreign-scratch')
            self.assertEqual((foreign.stat().st_dev, foreign.stat().st_ino), (initial.st_dev, initial.st_ino))
            self.assertIsNone(manager.dropin_parent)
            self.assertFalse(Path('/sys/fs/cgroup/system.slice', unit).exists())
        finally:
            os.close(reader)
            os.close(writer)
            manager.close()

    def remove_foreign(self, unit, path, directory, initial):
        self.assertFalse(Path('/sys/fs/cgroup/system.slice', unit).exists())
        self.assertEqual(path.read_bytes(), b'foreign-scratch')
        self.assertEqual((path.stat().st_dev, path.stat().st_ino), (initial.st_dev, initial.st_ino))
        path.unlink()
        directory.rmdir()


if __name__ == '__main__':
    if os.geteuid() != 0 or sys.platform != 'linux':
        raise SystemExit('actual root Linux manager qualification required')
    selection = ['CleanProfileTest']
    if sys.argv[1:] == ['--container']:
        selection.append('ContainerPolicyTest')
    elif sys.argv[1:]:
        raise SystemExit('invalid policy qualification mode')
    unittest.main(argv=[sys.argv[0], *selection])

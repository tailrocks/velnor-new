"""Adversarial cold fallback: restored bytes cannot grant warm authority."""
import hashlib
import importlib.util
import json
import os
import pathlib
import tempfile
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    'native_health', pathlib.Path(__file__).with_name('catalog_native_health.py'))
health = importlib.util.module_from_spec(spec)
spec.loader.exec_module(health)
from catalog_executable_bounds import executable_limit, stream_sha256
health.executable_limit = executable_limit
health.stream_sha256 = stream_sha256


class ColdAdmission(unittest.TestCase):
    def setUp(self):
        self.sandbox = tempfile.TemporaryDirectory()
        self.base = pathlib.Path(self.sandbox.name).resolve()
        self.root = self.base / 'mise'
        self.bin = self.root / 'bin'
        self.bin.mkdir(parents=True)
        self.manager = self.bin / 'mise'
        self.manager.write_bytes(b'qualified manager fixture')
        self.manager.chmod(0o700)
        self.digest = hashlib.sha256(self.manager.read_bytes()).hexdigest()

    def tearDown(self):
        self.sandbox.cleanup()

    def admit(self):
        health.cold_prepare(str(self.root), self.digest, str(self.base))

    def inject_device_mismatch(self, target):
        original_fstat = health.os.fstat
        target_inode = target.stat().st_ino

        def mismatched(fd):
            info = original_fstat(fd)
            if info.st_ino != target_inode:
                return info
            fields = list(info)
            fields[2] = info.st_dev + 1
            return os.stat_result(fields)

        return patch.object(health.os, 'fstat', side_effect=mismatched)

    def test_every_restored_namespace_removed(self):
        for name in ('installs', 'downloads', 'plugins', 'shims', 'cache',
                     'velnor-empty-config', '.velnor-integrity'):
            payload = self.root / name / 'nested'
            payload.mkdir(parents=True)
            (payload / 'untrusted').write_text('payload')
        (self.root / '.velnor-mise-receipt.json').write_text('forged')
        (self.bin / 'uv').write_text('untrusted dependency')
        self.admit()
        self.assertEqual(list(self.root.iterdir()), [self.bin])
        self.assertEqual(list(self.bin.iterdir()), [self.manager])
        self.assertEqual(self.manager.read_bytes(), b'qualified manager fixture')

    def test_marker_and_executable_replacement_never_retained(self):
        payload = self.root / 'installs' / 'node' / '24' / 'bin'
        payload.mkdir(parents=True)
        (payload / 'node').write_text('exact version impersonation')
        (self.root / 'manifest.json').write_text('replacement matching inventory')
        self.admit()
        self.assertFalse(payload.exists())

    def test_symlink_payload_does_not_delete_external_files(self):
        outside = self.base / 'outside'
        outside.mkdir()
        canary = outside / 'secret'
        canary.write_text('preserve')
        (self.root / 'installs').symlink_to(outside, target_is_directory=True)
        self.admit()
        self.assertEqual(canary.read_text(), 'preserve')
        self.assertFalse((self.root / 'installs').is_symlink())

    def test_nested_escaping_link_does_not_follow(self):
        installed = self.root / 'installs'
        installed.mkdir()
        (installed / 'escape').symlink_to(self.base, target_is_directory=True)
        self.admit()
        self.assertTrue(self.manager.exists())

    def test_foreign_device_nested_preflight_preserves_siblings(self):
        installed = self.root / 'installs'
        foreign = installed / 'foreign'
        foreign.mkdir(parents=True)
        (foreign / 'canary').write_text('preserve')
        sibling = installed / 'sibling'
        sibling.write_text('preserve')
        with self.inject_device_mismatch(foreign), self.assertRaisesRegex(
                ValueError, 'native_health_entry_device'):
            self.admit()
        self.assertTrue((foreign / 'canary').exists())
        self.assertEqual(sibling.read_text(), 'preserve')

    def test_foreign_device_root_preflight_preserves_siblings(self):
        installed = self.root / 'installs'
        installed.mkdir()
        (installed / 'canary').write_text('preserve')
        sibling = self.root / 'sibling'
        sibling.write_text('preserve')
        with self.inject_device_mismatch(self.root), self.assertRaisesRegex(
                ValueError, 'native_health_entry_device'):
            self.admit()
        self.assertTrue((installed / 'canary').exists())
        self.assertEqual(sibling.read_text(), 'preserve')

    def test_fifo_removal_is_bounded(self):
        os.mkfifo(self.root / 'fifo')
        self.admit()
        self.assertFalse((self.root / 'fifo').exists())

    def test_linked_root_rejected_without_mutation(self):
        alias = self.base / 'alias'
        alias.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(OSError):
            health.cold_prepare(str(alias), self.digest, str(self.base))
        self.assertTrue(self.manager.exists())

    def test_linked_binary_directory_rejected(self):
        alternative = self.base / 'binary'
        self.bin.rename(alternative)
        self.bin.symlink_to(alternative, target_is_directory=True)
        with self.assertRaises(OSError):
            self.admit()
        self.assertTrue((alternative / 'mise').exists())

    def test_linked_manager_rejected(self):
        alternative = self.base / 'manager'
        self.manager.rename(alternative)
        self.manager.symlink_to(alternative)
        with self.assertRaises(OSError):
            self.admit()

    def test_changed_manager_rejected_before_cleanup(self):
        restored = self.root / 'installs'
        restored.mkdir()
        self.manager.write_text('unqualified manager')
        with self.assertRaisesRegex(ValueError, 'manager_digest'):
            self.admit()
        self.assertTrue(restored.exists())

    def test_hardlinked_manager_rejected(self):
        os.link(self.manager, self.base / 'other-link')
        with self.assertRaisesRegex(ValueError, 'manager_shape'):
            self.admit()

    def test_writable_root_rejected(self):
        self.root.chmod(0o777)
        with self.assertRaisesRegex(ValueError, 'native_health_owner'):
            self.admit()

    def test_writable_temp_ancestor_rejected(self):
        self.base.chmod(0o777)
        with self.assertRaisesRegex(ValueError, 'native_health_owner'):
            self.admit()
        self.base.chmod(0o700)

    def test_root_outside_runner_temp_rejected(self):
        with self.assertRaisesRegex(ValueError, 'native_health_root'):
            health.cold_prepare(str(self.root), self.digest, str(self.root / 'foreign'))

    def test_nested_hardlink_removal_preserves_external_canary(self):
        outside = self.base / 'outside'
        outside.write_text('preserve')
        installed = self.root / 'installs'
        installed.mkdir()
        os.link(outside, installed / 'linked')
        self.admit()
        self.assertEqual(outside.read_text(), 'preserve')
        self.assertEqual(outside.stat().st_nlink, 1)

    def test_fixed_isolated_shell_with_quoted_path(self):
        quoted = self.base / "spaces and ' apostrophe"
        self.root.rename(quoted)
        source = (pathlib.Path(__file__).with_name('catalog_executable_bounds.py').read_text()
                  + '\n' + pathlib.Path(health.__file__).read_text())
        code = ('exec(' + json.dumps(source) + '); import sys; '
                'cold_prepare(sys.argv[1], sys.argv[2], sys.argv[3])')
        script = ("/usr/bin/python3 -I -S -c '" + code.replace("'", "'\\''")
                  + "' \"$root\" \"$sha\" \"${RUNNER_TEMP:?}\"")
        result = subprocess.run(['/bin/bash', '-c', script], check=False,
                                capture_output=True, text=True,
                                env={'root': str(quoted), 'sha': self.digest,
                                     'RUNNER_TEMP': str(self.base)})
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()

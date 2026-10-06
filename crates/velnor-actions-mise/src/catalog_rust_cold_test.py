"""Restored Rust payload and self-forged marker never permit execution."""
import json
import hashlib
import os
import pathlib
import subprocess
import tempfile
import unittest

DIRECTORY = pathlib.Path(__file__).parent
COMMON = ((DIRECTORY / 'catalog_executable_bounds.py').read_text() + '\n'
          + (DIRECTORY / 'catalog_native_health.py').read_text())
SOURCE = (DIRECTORY / 'catalog_rust_cold.py').read_text()
namespace = {}
exec(COMMON, namespace)
exec(SOURCE, namespace)
reset = namespace['rust_cold_prepare']


class RustColdAdmission(unittest.TestCase):
    def setUp(self):
        self.sandbox = tempfile.TemporaryDirectory()
        self.temp = pathlib.Path(self.sandbox.name).resolve()
        self.owned = self.temp / 'velnor'
        self.owned.mkdir()
        self.cargo = self.owned / 'cargo'
        self.rustup = self.owned / 'rustup'
        self.cargo.mkdir()
        self.rustup.mkdir()
        self.mise = self.owned / 'mise'
        (self.mise / 'bin').mkdir(parents=True)
        self.manager = self.mise / 'bin' / 'mise'
        self.manager.write_bytes(b'qualified mise fixture')
        self.manager.chmod(0o700)
        self.sha = hashlib.sha256(self.manager.read_bytes()).hexdigest()

    def tearDown(self):
        self.sandbox.cleanup()

    def prepare(self):
        reset(str(self.temp), str(self.cargo), str(self.rustup), self.sha)

    def test_whole_rustup_and_cargo_tools_removed_sources_preserved(self):
        for name in ('toolchains', 'downloads', 'tmp', 'update-hashes',
                     'velnor-integrity'):
            directory = self.rustup / name
            directory.mkdir()
            (directory / 'forged').write_text('forged')
        (self.rustup / 'settings.toml').write_text('attacker overrides')
        for name in ('bin',):
            (self.cargo / name).mkdir()
            (self.cargo / name / 'cargo').write_text('poisoned executable')
        for name in ('.crates.toml', '.crates2.json'):
            (self.cargo / name).write_text('forged install records')
        for name in ('registry/index', 'registry/cache', 'git/db'):
            directory = self.cargo / name
            directory.mkdir(parents=True)
            (directory / 'source').write_text('source bytes')
        self.prepare()
        self.assertEqual(list(self.rustup.iterdir()), [])
        for name in ('bin', '.crates.toml', '.crates2.json'):
            self.assertFalse((self.cargo / name).exists())
        for name in ('registry/index', 'registry/cache', 'git/db'):
            self.assertEqual((self.cargo / name / 'source').read_text(), 'source bytes')

    def test_forged_payload_marker_never_executes_in_actual_shell(self):
        binary = self.rustup / 'toolchains' / 'exact-host' / 'bin' / 'rustc'
        binary.parent.mkdir(parents=True)
        sentinel = self.temp / 'EXECUTED'
        binary.write_text('#!/bin/sh\ntouch "' + str(sentinel) + '"\n')
        binary.chmod(0o700)
        marker = self.rustup / 'velnor-integrity' / 'exact-host.sha256'
        marker.parent.mkdir()
        marker.write_text('forged payload-matching checksum')
        plugin = self.mise / 'plugins' / 'rust' / 'bin' / 'exec-env'
        plugin.parent.mkdir(parents=True)
        plugin.write_text('#!/bin/sh\ntouch "' + str(sentinel) + '"\n')
        plugin.chmod(0o700)
        wrapper = self.mise / 'command-wrappers' / 'bin' / 'cargo'
        wrapper.parent.mkdir(parents=True)
        wrapper.write_text(plugin.read_text())
        wrapper.chmod(0o700)
        (self.mise / 'config.toml').write_text('hostile plugin startup config')
        code = ('exec(' + json.dumps(COMMON) + '); exec(' + json.dumps(SOURCE)
                + '); import os; rust_cold_prepare(os.environ["RUNNER_TEMP"], '
                'os.environ["CARGO_HOME"], os.environ["RUSTUP_HOME"], ' + repr(self.sha) + ')')
        script = ("set -e; /usr/bin/python3 -I -S -c '"
                  + code.replace("'", "'\\''")
                  + "'; for candidate in \"$hostile\" \"$plugin\" \"$wrapper\"; do if test -e \"$candidate\"; then \"$candidate\"; fi; done")
        result = subprocess.run(['/bin/bash', '-c', script], capture_output=True,
                                text=True, check=False,
                                env={'RUNNER_TEMP': str(self.temp),
                                     'CARGO_HOME': str(self.cargo),
                                     'RUSTUP_HOME': str(self.rustup),
                                     'hostile': str(binary), 'plugin': str(plugin),
                                     'wrapper': str(wrapper)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(sentinel.exists())
        self.assertFalse(binary.exists())
        self.assertFalse(marker.exists())
        self.assertFalse(plugin.exists())
        self.assertFalse(wrapper.exists())
        self.assertFalse((self.mise / 'config.toml').exists())

    def test_proxy_hardlinks_removed_without_mutating_external_bytes(self):
        manager = self.temp / 'qualified-rustup'
        manager.write_bytes(b'qualified manager')
        (self.cargo / 'bin').mkdir()
        for name in ('rustup', 'cargo', 'rustc'):
            os.link(manager, self.cargo / 'bin' / name)
        self.prepare()
        self.assertEqual(manager.read_bytes(), b'qualified manager')
        self.assertEqual(manager.stat().st_nlink, 1)

    def test_payload_links_and_fifo_removed_without_following(self):
        outside = self.temp / 'outside'
        outside.mkdir()
        (outside / 'canary').write_text('preserve')
        (self.rustup / 'toolchains').symlink_to(outside, target_is_directory=True)
        (self.cargo / 'bin').symlink_to(outside, target_is_directory=True)
        os.mkfifo(self.rustup / 'fifo')
        self.prepare()
        self.assertEqual((outside / 'canary').read_text(), 'preserve')
        self.assertEqual(list(self.rustup.iterdir()), [])

    def test_foreign_homes_rejected(self):
        with self.assertRaisesRegex(ValueError, 'home_binding'):
            reset(str(self.temp), str(self.cargo), str(self.temp / 'foreign'), self.sha)

    def test_linked_root_rejected(self):
        self.rustup.rmdir()
        self.rustup.symlink_to(self.temp, target_is_directory=True)
        with self.assertRaises(OSError):
            self.prepare()

    def test_writable_parent_rejected(self):
        self.owned.chmod(0o777)
        with self.assertRaisesRegex(ValueError, 'native_health_owner'):
            self.prepare()
        self.owned.chmod(0o700)

    def test_unowned_cargo_configuration_fails_before_execution(self):
        (self.cargo / 'config.toml').write_text('rustc-wrapper="hostile"')
        with self.assertRaisesRegex(ValueError, 'unowned_cargo_configuration'):
            self.prepare()

    def test_missing_roots_created_empty(self):
        self.cargo.rmdir()
        self.rustup.rmdir()
        self.prepare()
        self.assertEqual(list(self.cargo.iterdir()), [])
        self.assertEqual(list(self.rustup.iterdir()), [])

    def test_mise_plugins_wrappers_config_removed_before_probes(self):
        sentinel = self.temp / 'MISE_EXECUTED'
        for name in ('plugins', 'installs', 'downloads', 'shims',
                     'command-wrappers/bin', 'velnor-empty-config'):
            directory = self.mise / name
            directory.mkdir(parents=True)
            hostile = directory / 'hostile'
            hostile.write_text('#!/bin/sh\ntouch "' + str(sentinel) + '"\n')
            hostile.chmod(0o700)
        (self.mise / 'config.toml').write_text('forged executable config')
        self.prepare()
        self.assertEqual(list(self.mise.iterdir()), [self.mise / 'bin'])
        self.assertEqual(list((self.mise / 'bin').iterdir()), [self.manager])
        self.assertFalse(sentinel.exists())

    def test_planning_bootstrap_missing_full_manager_clears_full_state(self):
        self.manager.unlink()
        (self.mise / 'plugins').mkdir()
        (self.mise / 'plugins' / 'hostile').write_text('untrusted')
        self.prepare()
        self.assertEqual(list(self.mise.iterdir()), [])

    def test_unqualified_unused_full_manager_is_discarded(self):
        self.manager.write_text('unqualified Full manager')
        self.prepare()
        self.assertEqual(list(self.mise.iterdir()), [])


if __name__ == '__main__':
    unittest.main()

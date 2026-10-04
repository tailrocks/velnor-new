"""Real source-bound helper materialization and launch regression tests."""
import hashlib
import importlib.util
import os
from pathlib import Path
import sys
import fcntl
import tempfile
import unittest
from unittest.mock import patch

PATH = Path(__file__).resolve().parents[1] / 'src' / 'source_helper_launcher.py'
SPEC = importlib.util.spec_from_file_location('source_helper_launcher', PATH)
HELPER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HELPER)


class HelperTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = str(Path(self.temp.name).resolve())
        self.environment = patch.dict(os.environ, {'RUNNER_TEMP': self.root})
        self.environment.start()

    def tearDown(self):
        self.environment.stop()
        self.temp.cleanup()

    def run_source(self, source, arguments=()):
        return HELPER.execute(source, hashlib.sha256(source.encode()).hexdigest(), arguments, [])

    def test_verified_bytes_run_and_file_is_unlinked(self):
        output = Path(self.root) / 'result'
        source = 'printf "%s" "$1" > "$2"\n'
        self.assertEqual(self.run_source(source, ['literal $() argument', str(output)]), 0)
        self.assertEqual(output.read_text(), 'literal $() argument')
        self.assertEqual([path.name for path in Path(self.root).iterdir()], ['result'])

    def test_digest_rejection_runs_nothing(self):
        output = Path(self.root) / 'result'
        with self.assertRaisesRegex(SystemExit, 'source_helper_digest'):
            HELPER.execute(f'touch "{output}"\n', '0' * 64, [], [])
        self.assertFalse(output.exists())

    def test_symlink_ancestor_rejected(self):
        real = Path(self.root) / 'actual'
        real.mkdir()
        link = Path(self.root) / 'alias'
        link.symlink_to(real, target_is_directory=True)
        with patch.dict(os.environ, {'RUNNER_TEMP': str(link)}):
            with self.assertRaises(OSError):
                self.run_source('exit 0\n')

    def test_parent_segments_and_writable_roots_rejected(self):
        for bad in [self.root + '/../escape', self.root + '//child', 'relative']:
            with self.assertRaisesRegex(SystemExit, 'source_helper_temp_path'):
                HELPER.temporary_root(bad)
        os.chmod(self.root, 0o777)
        with self.assertRaisesRegex(SystemExit, 'source_helper_temp_owner'):
            HELPER.temporary_root(self.root)

    def test_materialization_private_regular_unlinked_file(self):
        root = HELPER.temporary_root(self.root)
        descriptor, feeder = HELPER.materialize(b'exit 0\n')
        try:
            info = os.fstat(descriptor)
            if sys.platform == 'linux':
                self.assertEqual(info.st_mode & 0o777, 0o700)
                self.assertTrue(fcntl.fcntl(descriptor, fcntl.F_GET_SEALS) & fcntl.F_SEAL_WRITE)
            else:
                self.assertEqual(fcntl.fcntl(descriptor, fcntl.F_GETFL) & os.O_ACCMODE, os.O_RDONLY)
            self.assertEqual(info.st_nlink, 0)
            self.assertEqual(os.read(descriptor, 100), b'exit 0\n')
            self.assertEqual(list(Path(self.root).iterdir()), [])
        finally:
            HELPER.close_source(descriptor, feeder)
            os.close(root)

    def test_credentials_and_hooks_removed_before_execution(self):
        hook = Path(self.root) / 'hook'
        marker = Path(self.root) / 'hook-ran'
        hook.write_text(f'touch "{marker}"\n')
        secrets = {'BASH_ENV': str(hook), 'ENV': str(hook), 'GH_TOKEN': 'secret',
                   'CARGO_REGISTRIES_TEST_TOKEN': 'secret', 'ARBITRARY_TOKEN': 'secret'}
        with patch.dict(os.environ, secrets):
            source = 'test -z "${GH_TOKEN+x}${CARGO_REGISTRIES_TEST_TOKEN+x}${ARBITRARY_TOKEN+x}${BASH_ENV+x}${ENV+x}"\n'
            self.assertEqual(self.run_source(source), 0)
        self.assertFalse(marker.exists())

    def test_argument_boundaries_and_failure_preserved(self):
        self.assertEqual(self.run_source('test "$#" = 2 && test "$1" = "a b" && test "$2" = "*"\n', ['a b', '*']), 0)
        self.assertEqual(self.run_source('exit 37\n'), 37)

    def test_temp_path_replacement_does_not_redirect_materialization(self):
        root = HELPER.temporary_root(self.root)
        moved = self.root + '-moved'
        os.rename(self.root, moved)
        os.mkdir(self.root, 0o700)
        descriptor, feeder = HELPER.materialize(b'exit 0\n')
        try:
            self.assertEqual(os.read(descriptor, 100), b'exit 0\n')
            self.assertEqual(list(Path(self.root).iterdir()), [])
            self.assertEqual(list(Path(moved).iterdir()), [])
        finally:
            HELPER.close_source(descriptor, feeder)
            os.close(root)
            os.rmdir(moved)

    def test_source_descriptor_cannot_be_written(self):
        descriptor, feeder = HELPER.materialize(b'exit 0\n')
        try:
            with self.assertRaises(OSError):
                os.write(descriptor, b'evil')
            if sys.platform == 'linux':
                alias = os.open('/proc/self/fd/' + str(descriptor), os.O_RDWR)
                try:
                    with self.assertRaises(OSError):
                        os.write(alias, b'evil')
                finally:
                    os.close(alias)
        finally:
            HELPER.close_source(descriptor, feeder)

    def test_large_source_and_early_exit_do_not_hang_pipe(self):
        source = 'exit 37\n' + '# unused\n' * 12000
        self.assertEqual(self.run_source(source), 37)

    def test_oversize_source_and_arguments_fail_before_execution(self):
        with self.assertRaisesRegex(SystemExit, 'source_helper_digest'):
            self.run_source('#' * 262145)
        with self.assertRaisesRegex(SystemExit, 'source_helper_argument_size'):
            limit = 131072 if sys.platform == 'linux' else os.sysconf('SC_ARG_MAX')
            self.run_source('exit 0\n', ['x' * limit])


if __name__ == '__main__':
    unittest.main()

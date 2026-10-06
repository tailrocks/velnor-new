"""Rust native producer command and containment regressions."""
import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import native_ffi as ffi


class NativeFfiTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        cwd = patch.object(ffi.Path, 'cwd', return_value=self.root)
        cwd.start()
        self.addCleanup(cwd.stop)
        self.crate = self.root / 'components/orbit-bridge'
        self.crate.mkdir(parents=True)
        self.manifest = self.crate / 'Cargo.toml'
        self.manifest.write_text('[package]\nname = "orbit-bridge"\nversion = "0.1.0"\n')
        (self.crate / 'boltffi.toml').write_text('[package]\ncrate = "orbit-bridge"\n')
        self.sha = 'abcde' * 8
        self.request = {
            'source_root': '.', 'source_sha': self.sha,
            'manifest_path': 'components/orbit-bridge/Cargo.toml',
            'package': 'orbit-bridge', 'profile': 'native-release',
            'features': ['alpha', 'native'], 'static_library': 'liborbit_bridge.a',
            'output_root': '.velnor-rust-ffi/orbit-bridge', 'deployment_target': '25.3',
            'compile_driver': 'cargo',
            'framework_name': 'OrbitCore', 'module_name': 'OrbitCoreFFI',
            'profile_digest': 'b3-' + '0' * 64,
        }
        self.output = self.root / self.request['output_root']
        self.library = self.root / 'target/aarch64-apple-darwin/native-release/liborbit_bridge.a'
        self.library.parent.mkdir(parents=True)
        self.library.write_bytes(b'compiled library')

    def artifact(self, manifest=None, library=None):
        return json.dumps({'reason': 'compiler-artifact',
                           'manifest_path': str(manifest or self.manifest),
                           'package_id': 'path+file://' + str(self.crate) + '#orbit-bridge@0.1.0',
                           'target': {'name': 'orbit_bridge', 'kind': ['staticlib'],
                                      'crate_types': ['staticlib']},
                           'filenames': [str(library or self.library)]}) + '\n'

    def stale_output(self):
        (self.output / 'bindings').mkdir(parents=True)
        (self.output / 'bindings/Stale.swift').write_text('stale')
        (self.output / 'library').mkdir()
        (self.output / 'library/liborbit_bridge.a').write_bytes(b'stale')
        (self.output / 'artifacts.json').write_text('{"stale": true}')

    def command(self, argv, cwd=None, env=None):
        if argv[0] == '/usr/bin/git':
            return self.sha + '\n'
        if argv[:2] in (['cargo', 'build'], ['mbx', 'build']):
            return self.artifact()
        if argv[0] == 'boltffi' and argv[-2:] == ['generate', 'swift']:
            config = Path(argv[argv.index('--overlay') + 1])
            destination = Path(tomllib.loads(config.read_text())['targets']['apple']['swift']['output'])
            destination.mkdir(parents=True, exist_ok=True)
            (destination / 'Orbit.swift').write_text('struct Orbit {}\n')
            (destination / 'BoltFFI').mkdir(exist_ok=True)
            (destination / 'BoltFFI/boltffi.h').write_text('header\n')
        return ''

    def test_paths_reject_traversal_absolute_and_symlink_components(self):
        outside = self.root.parent / (self.root.name + '-outside')
        (self.root / 'alias').symlink_to(outside, target_is_directory=True)
        for value in ['../escape', '/etc/passwd', 'a/../b', 'a/./b', 'a//b',
                      'a\\b', 'alias/file']:
            with self.subTest(value=value), self.assertRaises(ValueError):
                ffi.safe_path(self.root, value)
        self.assertEqual(ffi.safe_path(self.root, 'build/library.a'), self.root / 'build/library.a')

    def test_run_preserves_argv_and_fails_on_command_error(self):
        complete = subprocess.CompletedProcess(['tool'], 0, stdout='ok')
        with patch.object(ffi.subprocess, 'run', return_value=complete) as command, patch('builtins.print'):
            self.assertEqual(ffi.run(['tool', 'argument with spaces'], cwd=self.root, env={}), 'ok')
        self.assertEqual(command.call_args.args[0], ['tool', 'argument with spaces'])
        self.assertTrue(command.call_args.kwargs['check'])
        self.assertEqual(command.call_args.kwargs['stderr'], subprocess.STDOUT)
        self.assertNotIn('shell', command.call_args.kwargs)
        failure = subprocess.CalledProcessError(1, ['tool'])
        with patch.object(ffi.subprocess, 'run', side_effect=failure), self.assertRaises(subprocess.CalledProcessError):
            ffi.run(['tool'], cwd=self.root, env={})

    def test_request_rejects_open_driver_features_and_paths_before_producer(self):
        cases = [('compile_driver', 'cargo xtask'), ('compile_driver', '--help'),
                 ('features', ['native', 'native']), ('features', ['native', 'alpha']),
                 ('features', ['--locked']), ('features', ['a/b']),
                 ('manifest_path', '../Cargo.toml'), ('output_root', '../build'),
                 ('static_library', '../liborbit_bridge.a'), ('source_sha', 'abc')]
        for name, value in cases:
            request = copy.deepcopy(self.request)
            request[name] = value
            with self.subTest(name=name, value=value), patch.object(ffi, 'run', side_effect=self.command) as command:
                with self.assertRaises(ValueError):
                    ffi.produce(request)
                self.assertTrue(all(call.args[0][0] == '/usr/bin/git' for call in command.call_args_list))

    def test_environment_removes_credentials_and_sets_owned_target_directory(self):
        supplied = {'PATH': '/tools', 'GITHUB_TOKEN': 'token', 'GH_TOKEN': 'token',
                    'GITHUB_OUTPUT': '/runner/output', 'ACTIONS_RUNTIME_TOKEN': 'token',
                    'RUNNER_TEMP': '/runner/temp', 'CUSTOM_API_KEY': 'secret',
                    'CARGO_TARGET_DIR': '/unowned', 'MACOSX_DEPLOYMENT_TARGET': '1.0'}
        with patch.dict(os.environ, supplied, clear=True):
            environment = ffi.environment(self.root, '25.3')
        self.assertEqual(environment, {'PATH': '/tools', 'CARGO_TARGET_DIR': str(self.root / 'target'),
                                       'MACOSX_DEPLOYMENT_TARGET': '25.3'})

    def test_source_sha_mismatch_stops_before_producer(self):
        self.stale_output()
        with patch.object(ffi, 'run', return_value='0' * 40 + '\n') as command:
            with self.assertRaises(ValueError):
                ffi.produce(self.request)
        self.assertEqual(command.call_count, 1)
        self.assertEqual(command.call_args.args[0],
                         ['/usr/bin/git', '-C', self.root, 'rev-parse', '--verify', 'HEAD'])
        self.assertFalse(self.output.exists())

    def test_missing_manifest_removes_stale_receipt_before_failure(self):
        self.stale_output()
        self.manifest.unlink()
        with patch.object(ffi, 'run', side_effect=self.command) as command:
            with self.assertRaisesRegex(ValueError, 'missing Rust FFI producer manifest'):
                ffi.produce(self.request)
        self.assertEqual(command.call_count, 1)
        self.assertEqual(command.call_args.args[0][0], '/usr/bin/git')
        self.assertFalse(self.output.exists())

    def test_nested_boltffi_pack_symlink_removes_stale_receipt_before_commands(self):
        self.stale_output()
        pack = self.root / 'target/boltffi/pack'
        pack.mkdir(parents=True)
        (pack / 'apple').symlink_to(self.crate, target_is_directory=True)
        with patch.object(ffi, 'run') as command, self.assertRaisesRegex(ValueError, 'symlink'):
            ffi.produce(self.request)
        command.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_unowned_output_cannot_delete_source_tree(self):
        for destination in ['components/orbit-bridge', '.velnor-rust-ffi/other-package']:
            request = dict(self.request, output_root=destination)
            victim = self.root / destination
            victim.mkdir(parents=True, exist_ok=True)
            marker = victim / 'keep'
            marker.write_text('preserve')
            with self.subTest(destination=destination), patch.object(ffi, 'run') as command:
                with self.assertRaisesRegex(ValueError, 'unowned'):
                    ffi.produce(request)
                command.assert_not_called()
            self.assertEqual(marker.read_text(), 'preserve')

    def test_cargo_and_boltffi_commands_are_fixed_locked_and_keep_default_features(self):
        with patch.object(ffi, 'run', side_effect=self.command) as command:
            ffi.produce(self.request)
        calls = [call.args[0] for call in command.call_args_list]
        cargo = next(argv for argv in calls if argv[:2] == ['cargo', 'build'])
        self.assertEqual(cargo, ['cargo', 'build', '--locked', '--manifest-path', self.manifest,
                                '--profile', 'native-release', '--target', 'aarch64-apple-darwin',
                                '--package', 'orbit-bridge', '--message-format=json-render-diagnostics',
                                '--features', 'alpha', '--features', 'native'])
        bolt_calls = [argv for argv in calls if argv[0] == 'boltffi']
        self.assertEqual(len(bolt_calls), 2)
        common = ['boltffi', '--cargo-arg=--locked', '--cargo-arg=--profile',
                  '--cargo-arg=native-release', '--cargo-arg=--features', '--cargo-arg=alpha',
                  '--cargo-arg=--features', '--cargo-arg=native']
        generate, build = bolt_calls
        self.assertEqual(generate[:len(common)], common)
        self.assertEqual(generate[len(common):len(common) + 2],
                         ['--cargo-arg=--target', '--cargo-arg=aarch64-apple-darwin'])
        self.assertEqual(generate[-4], '--overlay')
        self.assertEqual(generate[-2:], ['generate', 'swift'])
        self.assertEqual(build, common + ['--overlay', generate[-3], 'build', 'apple'])
        self.assertFalse(any('no-default-features' in str(item) for argv in calls for item in argv))
        self.assertEqual((self.output / 'library/liborbit_bridge.a').read_bytes(), b'compiled library')
        self.assertTrue((self.output / 'bindings/Orbit.swift').is_file())
        receipt = json.loads((self.output / 'artifacts.json').read_text())
        self.assertEqual(receipt, {
            'schema': 1, 'producer_kind': 'library',
            'generated_header': '.velnor-rust-ffi/orbit-bridge/bindings/BoltFFI/boltffi.h',
            'header_namespace': 'orbit-bridge', 'module_name': 'OrbitCoreFFI',
            'library_path': '.velnor-rust-ffi/orbit-bridge/library/liborbit_bridge.a',
            'bindings_path': '.velnor-rust-ffi/orbit-bridge/bindings', 'source_sha': self.sha,
            'profile_digest': self.request['profile_digest'],
            'hashes': {'bindings/Orbit.swift': hashlib.sha256(b'struct Orbit {}\n').hexdigest(),
                       'bindings/BoltFFI/boltffi.h': hashlib.sha256(b'header\n').hexdigest(),
                       'library/liborbit_bridge.a': hashlib.sha256(b'compiled library').hexdigest()},
        })

    def test_failed_producer_cannot_leave_previous_library_or_bindings(self):
        self.stale_output()
        old = self.output / 'library/liborbit_bridge.a'
        def fail(argv, cwd=None, env=None):
            if argv[:2] == ['cargo', 'build']:
                raise subprocess.CalledProcessError(1, argv)
            return self.command(argv, cwd, env)
        with patch.object(ffi, 'run', side_effect=fail), self.assertRaises(subprocess.CalledProcessError):
            ffi.produce(self.request)
        self.assertFalse(old.exists())
        self.assertFalse((self.output / 'bindings/Stale.swift').exists())
        self.assertFalse((self.output / 'artifacts.json').exists())

    def test_bindings_only_producer_has_no_library_or_build_commands(self):
        with patch.object(ffi, 'run', side_effect=self.command) as command:
            ffi.produce(self.request, build=False)
        calls = [call.args[0] for call in command.call_args_list]
        self.assertEqual([argv[-2:] for argv in calls if argv[0] == 'boltffi'], [['generate', 'swift']])
        self.assertFalse(any(argv[0] in ('cargo', 'mbx', 'rustup') for argv in calls))
        receipt = json.loads((self.output / 'artifacts.json').read_text())
        self.assertEqual(receipt['producer_kind'], 'bindings')
        self.assertIsNone(receipt['library_path'])
        self.assertFalse((self.output / 'library').exists())

    def test_generated_symlink_or_multiple_headers_cannot_publish(self):
        for invalid in ['symlink', 'multiple-headers']:
            def generate(argv, cwd=None, env=None):
                result = self.command(argv, cwd, env)
                if argv[0] == 'boltffi' and argv[-2:] == ['generate', 'swift']:
                    config = Path(argv[argv.index('--overlay') + 1])
                    generated = Path(tomllib.loads(config.read_text())['targets']['apple']['swift']['output'])
                    if invalid == 'symlink':
                        (generated / 'escape.swift').symlink_to(self.manifest)
                    else:
                        (generated / 'extra.h').write_text('extra')
                return result
            with self.subTest(invalid=invalid), patch.object(ffi, 'run', side_effect=generate):
                with self.assertRaises(ValueError):
                    ffi.produce(self.request)
            self.assertFalse(self.output.exists())

    def test_cargo_artifacts_reject_duplicates_escape_and_symlinks(self):
        alias = self.library.with_name('alias')
        alias.symlink_to(self.library.parent, target_is_directory=True)
        cases = [self.artifact() * 2, self.artifact(library=Path('/tmp/liborbit_bridge.a')),
                 self.artifact(library=alias / self.library.name)]
        for output in cases:
            def invalid(argv, cwd=None, env=None):
                return output if argv[:2] == ['cargo', 'build'] else self.command(argv, cwd, env)
            with self.subTest(output=output), patch.object(ffi, 'run', side_effect=invalid):
                with self.assertRaises(ValueError):
                    ffi.produce(self.request)
            self.assertFalse(self.output.exists())

    def test_artifact_from_other_manifest_is_rejected(self):
        def foreign(argv, cwd=None, env=None):
            if argv[:2] == ['cargo', 'build']:
                return self.artifact(manifest=self.root / 'other/Cargo.toml')
            return self.command(argv, cwd, env)
        with patch.object(ffi, 'run', side_effect=foreign), self.assertRaises(ValueError):
            ffi.produce(self.request)
        self.assertFalse((self.output / 'library/liborbit_bridge.a').exists())

    def test_artifacts_reject_same_named_source_host_and_nested_libraries(self):
        locations = ['checked-in/liborbit_bridge.a', 'target/native-release/liborbit_bridge.a',
                     'target/x86_64-apple-darwin/native-release/liborbit_bridge.a',
                     'target/aarch64-apple-darwin/native-release/deps/liborbit_bridge.a']
        for location in locations:
            library = self.root / location
            library.parent.mkdir(parents=True, exist_ok=True)
            library.write_bytes(b'unselected library')
            self.stale_output()
            def wrong_artifact(argv, cwd=None, env=None):
                return self.artifact(library=library) if argv[:2] == ['cargo', 'build'] else self.command(argv, cwd, env)
            with self.subTest(location=location), patch.object(ffi, 'run', side_effect=wrong_artifact):
                with self.assertRaises(ValueError):
                    ffi.produce(self.request)
            self.assertFalse(self.output.exists())

    def test_library_tests_use_explicit_packages_and_closed_driver(self):
        request = {'source_root': '.', 'source_sha': self.sha,
                   'packages': ['orbit-bridge', 'orbit-core'], 'compile_driver': 'mbx'}
        with patch.object(ffi, 'run', side_effect=self.command) as command:
            ffi.library_tests(request)
        self.assertEqual(command.call_args.args[0], ['mbx', 'nextest', 'run', '--locked', '--lib',
                                                    '--package', 'orbit-bridge', '--package', 'orbit-core'])
        self.assertEqual(command.call_args.args[1], self.root)
        for driver in ['arbitrary', 'cargo build']:
            request['compile_driver'] = driver
            with patch.object(ffi, 'run') as command, self.assertRaises(ValueError):
                ffi.library_tests(request)
            command.assert_not_called()


if __name__ == '__main__':
    unittest.main()

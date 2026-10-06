"""Native primitive regression and adversarial qualification tests."""
import copy
import json
import os
from pathlib import Path
import plistlib
import py_compile
import stat
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import desktop_native as entry
import desktop_native_build as build
import desktop_native_core as core
import desktop_native_verify as verify


def orbit():
    return {'native_root': 'clients/orbit', 'target': 'apple-arm64', 'deployment_target': '25.3',
            'ffi': {'framework_name': 'OrbitCore',
                    'module_name': 'OrbitCoreFFI', 'static_library': 'liborbit_bridge.a',
                    'bindings_path': 'clients/orbit/generated/orbit-bindings',
                    'xcframework_path': 'build/OrbitCore.xcframework'},
            'apple': {'project_spec': 'native.yaml', 'project_path': 'Orbit.xcodeproj',
                      'scheme': 'OrbitApp', 'app_name': 'OrbitApp', 'bundle_identifier': 'org.example.orbit',
                      'bundle_name': 'Orbit Desktop', 'app_path': 'build/OrbitApp.app',
                      'derived_data_path': 'build/orbit-data', 'archive_name_prefix': 'orbit-desktop',
                      'required_resources': ['Contents/Resources/Assets.car'], 'bundle_lsui_element': False}}

class NativeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()
        (self.root / 'clients/orbit').mkdir(parents=True)
        self.profile = orbit()
        self.profile['_root'] = self.root

    def tearDown(self):
        self.temporary.cleanup()

    def load(self, profile):
        previous = Path.cwd()
        try:
            os.chdir(self.root)
            return core.load_profile_source(json.dumps(profile))
        finally:
            os.chdir(previous)

    def app(self):
        app = core.path(self.profile, 'apple.app_path')
        resource = app / 'Contents/Resources/Assets.car'
        resource.parent.mkdir(parents=True)
        resource.write_bytes(b'assets')
        executable = app / 'Contents/MacOS/OrbitApp'
        executable.parent.mkdir()
        executable.write_bytes(b'macho')
        info = {'CFBundleIdentifier': 'org.example.orbit', 'CFBundleExecutable': 'OrbitApp',
                'CFBundleName': 'Orbit Desktop', 'CFBundleShortVersionString': '1.2.3',
                'CFBundleVersion': '42', 'LSMinimumSystemVersion': '25.3', 'LSUIElement': False}
        (app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
        return app

    def test_distinct_profile_paths_resolve_native_project(self):
        profile = self.load(orbit())
        self.assertEqual(core.path(profile, 'apple.project_spec'), self.root / 'clients/orbit/native.yaml')
        self.assertEqual(core.path(profile, 'apple.app_path'), self.root / 'build/OrbitApp.app')
        with patch.object(build, 'run', return_value='') as run:
            build.generate_project(profile)
        argv = run.call_args.args[0]
        self.assertEqual(argv, ['xcodegen', 'generate', '--spec', self.root / 'clients/orbit/native.yaml'])
        self.assertEqual(run.call_args.kwargs['cwd'], self.root / 'clients/orbit')
        self.assertIn('OrbitApp', build.xcode_arguments(profile))
        self.assertIn(self.root / 'clients/orbit/Orbit.xcodeproj', build.xcode_arguments(profile))
        self.assertIn(self.root / 'build/orbit-data', build.xcode_arguments(profile))

    def test_swift_only_root_profile(self):
        profile = orbit()
        profile.pop('apple')
        profile['native_root'] = '.'
        parsed = self.load(profile)
        self.assertIsNone(parsed['apple'])
        self.assertEqual(core.path(parsed, 'native_root'), self.root)

    def test_consumer_paths_reject_escape_and_unknown_profile_fields(self):
        for field, value in [('bindings_path', '/tmp/escape'),
                             ('xcframework_path', 'a/../OrbitCore.xcframework')]:
            candidate = orbit()
            candidate['ffi'][field] = value
            with self.assertRaises(ValueError):
                self.load(candidate)
        candidate = orbit()
        candidate['command'] = 'arbitrary command'
        with self.assertRaises(ValueError):
            self.load(candidate)

    def test_consumer_projection_rejects_producer_fields_and_missing_identity(self):
        for field in ['manifest_path', 'package', 'profile', 'features', 'default_features']:
            candidate = orbit()
            candidate['ffi'][field] = 'producer-only'
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.load(candidate)
        for field in orbit()['ffi']:
            candidate = orbit()
            del candidate['ffi'][field]
            with self.subTest(missing=field), self.assertRaises(ValueError):
                self.load(candidate)
        candidate = orbit()
        candidate['checks'] = {'source_dirs': ['Sources'], 'cargo_test_packages': ['foreign']}
        with self.assertRaises(ValueError):
            self.load(candidate)

    def test_app_resource_paths_have_their_own_filesystem_domain(self):
        (self.root / 'Contents').symlink_to('/tmp', target_is_directory=True)
        self.load(orbit())
        candidate = orbit()
        candidate['apple']['required_resources'] = ['Contents/Resources/../escape']
        with self.assertRaises(ValueError):
            self.load(candidate)
        app = self.app()
        resource = app / 'Contents/Resources/Assets.car'
        resource.unlink()
        resource.symlink_to(self.root / 'profile.json')
        with self.assertRaisesRegex(ValueError, 'symlink'):
            verify.check_plist(self.profile, app, '1.2.3', '42')

    def test_symlink_and_traversal_fail(self):
        (self.root / 'alias').symlink_to('/tmp', target_is_directory=True)
        for value in ['alias/app', '../escape', 'a//b', 'a/./b', 'a\\b', '/etc/passwd']:
            with self.assertRaises(ValueError):
                core.safe_path(self.root, value)
        app = self.app()
        (app / 'Contents/Resources/extra').symlink_to('/tmp')
        with self.assertRaises(ValueError):
            verify.check_embedded(app)



    def test_plist_and_resources_match_distinct_profile(self):
        app = self.app()
        verify.check_plist(self.profile, app, '1.2.3', '42')
        with self.assertRaises(ValueError):
            verify.check_plist(self.profile, app, '1.2.4', '42')
        (app / 'Contents/Resources/Assets.car').unlink()
        with self.assertRaises(ValueError):
            verify.check_plist(self.profile, app, '1.2.3', '42')

    def test_architecture_vtool_and_linkage_fail_closed(self):
        binary = self.root / 'binary'
        binary.write_bytes(b'binary')
        valid = ['arm64\n', '    minos 25.3\n', 'binary:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1)\n']
        with patch.object(verify, 'run', side_effect=valid):
            verify.check_binary(self.profile, binary)
        cases = [['arm64 x86_64\n'], ['arm64\n', 'no deployment info'],
                 ['arm64\n', '    minos 25.2\n'],
                 ['arm64\n', '    minos 25.3\n', 'binary:\n\t/tmp/liborbit_bridge.dylib (compatibility version 1)']]
        for outputs in cases:
            with patch.object(verify, 'run', side_effect=outputs), self.assertRaises(ValueError):
                verify.check_binary(self.profile, binary)
        with patch.object(verify, 'run', side_effect=['arm64\n', subprocess.CalledProcessError(1, ['vtool'])]):
            with self.assertRaises(subprocess.CalledProcessError):
                verify.check_binary(self.profile, binary)

    def test_zip_rejects_traversal_symlink_and_multiple_apps(self):
        for index, names in enumerate([['../escape'], ['OrbitApp.app/Contents/a', 'Other.app/Contents/b'],
                                       ['/OrbitApp.app/Contents/a'], ['OrbitApp.app/Contents/a\\b']]):
            archive = self.root / f'{index}.zip'
            with zipfile.ZipFile(archive, 'w') as handle:
                for name in names:
                    handle.writestr(name, b'bytes')
            with self.assertRaises(ValueError):
                verify.archive_inventory(archive, 'OrbitApp')
        archive = self.root / 'symlink.zip'
        with zipfile.ZipFile(archive, 'w') as handle:
            item = zipfile.ZipInfo('OrbitApp.app/Contents/link')
            item.external_attr = (stat.S_IFLNK | 0o777) << 16
            handle.writestr(item, '/tmp')
        with self.assertRaises(ValueError):
            verify.archive_inventory(archive, 'OrbitApp')
        archive = self.root / 'valid.zip'
        with zipfile.ZipFile(archive, 'w') as handle:
            handle.writestr('OrbitApp.app/Contents/Info.plist', b'plist')
        self.assertEqual(verify.archive_inventory(archive, 'OrbitApp'), 'OrbitApp.app')

    def test_zip_bytes_bind_to_approved_app(self):
        approved = self.app()
        signature = approved / 'Contents/_CodeSignature/CodeResources'
        signature.parent.mkdir()
        signature.write_bytes(b'approved certificate seal')
        archive = self.root / 'archive.zip'
        with zipfile.ZipFile(archive, 'w') as handle:
            for relative, content in build.files(approved).items():
                handle.writestr('OrbitApp.app/' + relative, content)
        def extract(argv, **kwargs):
            destination = Path(argv[-1])
            with zipfile.ZipFile(archive) as handle:
                handle.extractall(destination)
            return ''
        with patch.object(verify, 'run', side_effect=extract), patch.object(verify, 'verify_app') as check:
            verify.verify_zip(self.profile, archive, '1.2.3', '42', True, approved)
        self.assertTrue(check.call_args.kwargs['release'])
        for altered in ['Contents/MacOS/OrbitApp', 'Contents/Resources/Assets.car',
                        'Contents/_CodeSignature/CodeResources']:
            def tampered(argv, **kwargs):
                extract(argv, **kwargs)
                (Path(argv[-1]) / 'OrbitApp.app' / altered).write_bytes(b'unapproved bytes')
                return ''
            with patch.object(verify, 'run', side_effect=tampered), patch.object(verify, 'verify_app') as check:
                with self.assertRaisesRegex(ValueError, 'ZIP bytes differ'):
                    verify.verify_zip(self.profile, archive, '1.2.3', '42', True, approved)
                check.assert_not_called()

    def test_derived_products_symlink_is_rejected_before_copy(self):
        derived = core.path(self.profile, 'apple.derived_data_path')
        derived.mkdir(parents=True)
        (derived / 'Build').symlink_to(self.root / 'clients', target_is_directory=True)
        with patch.object(build, 'assemble_framework'), patch.object(build, 'generate_project'), \
                patch.object(build, 'run', return_value='') as command:
            with self.assertRaisesRegex(ValueError, 'symlink'):
                build.build_app(self.profile, '1.2.3', '42')
        self.assertEqual(command.call_count, 1)
        self.assertEqual(command.call_args.args[0][0], 'xcodebuild')

    def test_managed_profile_and_nested_source_have_separate_roots(self):
        source = self.root / 'producer'
        (source / 'clients/orbit').mkdir(parents=True)
        config = self.root / '.github/velnor/desktop/profile.json'
        config.parent.mkdir(parents=True)
        config.write_text(json.dumps(orbit()))
        previous = Path.cwd()
        try:
            os.chdir(self.root)
            parsed = core.load_profile_source(config.read_text(), 'producer')
            self.assertEqual(parsed['_root'], source)
            self.assertEqual(core.path(parsed, 'ffi.bindings_path'), source / 'clients/orbit/generated/orbit-bindings')
            with self.assertRaises(ValueError):
                core.load_profile_source(config.read_text(), '../producer')
        finally:
            os.chdir(previous)

    def test_sealed_source_parser_never_reads_checkout_profile(self):
        source = json.dumps(orbit())
        (self.root / 'profile.json').write_text('{"command":"changed after sealing"}')
        with patch.object(core.Path, 'cwd', return_value=self.root), \
                patch.object(core.Path, 'read_text', side_effect=AssertionError('checkout reread')):
            parsed = core.load_profile_source(source)
        self.assertEqual(parsed['ffi'], orbit()['ffi'])
        self.assertFalse(hasattr(core, 'load_profile'))
        for invalid in [None, b'{}', '{}', '# Generated by unexpected tool\n' + source]:
            with patch.object(core.Path, 'cwd', return_value=self.root), self.assertRaises(ValueError):
                core.load_profile_source(invalid)

    def test_counted_swift_tests_reject_false_green(self):
        report = self.root / 'swift.xml'
        for body in ['<testsuites/>', '<testsuite tests="0" failures="0" errors="0"/>',
                     '<testsuite tests="3" failures="0"/>', '<testsuite tests="3" failures="1" errors="0"/>']:
            report.write_text(body)
            with self.assertRaises(ValueError):
                entry.swift_testing_totals(report)
        report.write_text('<testsuite tests="3" failures="0" errors="0"/>')
        entry.swift_testing_totals(report)

    def test_ui_test_requires_explicit_typed_target(self):
        native = core.path(self.profile, 'native_root') / 'UITests'
        native.mkdir(parents=True)
        (native / 'Smoke.swift').write_text('class Smoke: XCTestCase {\n func testSmoke() {}\n}\n')
        self.profile['apple']['ui_test_sources'] = ['UITests']
        with self.assertRaisesRegex(ValueError, 'explicit typed ui_test_target'):
            entry.selected_tests(self.profile, 'UITests')

    def test_xcode_results_require_exact_count_and_no_warning(self):
        result = self.root / 'results.xcresult'
        result.mkdir()
        (result / 'Info.plist').write_bytes(b'plist')
        valid = json.dumps({'totalTestCount': 1, 'failedTests': 0, 'passedTests': 1})
        with patch.object(entry, 'run', side_effect=[valid, '{}']):
            entry.result_summary(result, 1)
        with patch.object(entry, 'run', side_effect=[valid, '{"nodeType":"Runtime Warning"}']):
            with self.assertRaises(ValueError):
                entry.result_summary(result, 1)
        with patch.object(entry, 'run', return_value='{"totalTestCount":0,"failedTests":0,"passedTests":0}'):
            with self.assertRaises(ValueError):
                entry.result_summary(result, 1)

    def test_repo_tools_cannot_write_actions_outputs_or_use_tokens(self):
        envelope = {'PATH': '/tools', 'MACOSX_DEPLOYMENT_TARGET': '25.3',
                    'GITHUB_OUTPUT': '/runner/output', 'GITHUB_ENV': '/runner/env',
                    'GITHUB_STEP_SUMMARY': '/runner/summary', 'GITHUB_TOKEN': 'github',
                    'GH_TOKEN': 'gh', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'oidc',
                    'ACTIONS_RUNTIME_TOKEN': 'runtime', 'RUNNER_TEMP': '/runner/tmp',
                    'APP_STORE_CONNECT_API_KEY_PATH': '/secret/key',
                    'DEVELOPER_ID_APPLICATION': 'signing identity', 'CUSTOM_API_KEY': 'private'}
        expected = {'PATH': '/tools', 'MACOSX_DEPLOYMENT_TARGET': '25.3'}
        self.assertEqual(core.subprocess_environment(envelope), expected)
        trusted = core.subprocess_environment(envelope, trusted=True)
        self.assertEqual(trusted, dict(expected, GH_TOKEN='gh', GITHUB_TOKEN='github'))
        completed = subprocess.CompletedProcess(['swift'], 0, stdout='ok')
        with patch.object(core.subprocess, 'run', return_value=completed) as command:
            core.run(['swift', 'build'], env=envelope)
        self.assertEqual(command.call_args.kwargs['env'], expected)

    def test_admitted_sha_is_required_and_checked_before_repo_tools(self):
        expected = 'abcde' * 8
        for malformed in [None, '', 'abc', expected.upper(), expected + '0', '../escape']:
            with patch.object(core, 'run') as command:
                with self.assertRaises(ValueError):
                    core.validate_source_sha(self.profile, malformed)
                command.assert_not_called()
        with patch.object(core, 'run', return_value='0' * 40 + '\n'):
            with self.assertRaisesRegex(ValueError, 'differs from admitted SHA'):
                core.validate_source_sha(self.profile, expected)
        with patch.object(core, 'run', return_value=expected + '\n') as command:
            core.validate_source_sha(self.profile, expected)
        self.assertEqual(command.call_args.args[0], ['/usr/bin/git', '-C', self.root, 'rev-parse', '--verify', 'HEAD'])
        with patch.object(entry.sys, 'argv', ['desktop_native.py', 'xcframework', '--profile', 'profile.json']), \
                patch.object(entry, 'execute') as operation:
            with self.assertRaises(SystemExit):
                entry.main()
            operation.assert_not_called()

    def test_typed_checks_and_generated_marker(self):
        profile = orbit()
        profile['checks'] = {'source_dirs': ['Sources'], 'format_config': '.swift-format',
                             'swift_test_frameworks': ['xctest'],                              'swift_harness_products': ['OrbitHarness']}
        parsed = self.load(profile)
        config = self.root / 'profile.json'
        body = json.dumps(profile)
        config.write_text('# Generated by Velnor Actions 0.1.0; edit .velnor/config.toml and regenerate.\n' + body)
        with patch.object(core.Path, 'cwd', return_value=self.root):
            parsed = core.load_profile_source(config.read_text())
        with patch.object(entry, 'run', return_value='') as command:
            entry.swift_harnesses(parsed)
        self.assertEqual(command.call_args.args[0], ['swift', 'run', '-c', 'release', 'OrbitHarness'])
        source = self.root / 'clients/orbit/Sources'
        source.mkdir()
        (source / 'Orbit.swift').write_text('struct Orbit {}')
        with patch.object(entry, 'run', return_value='') as command:
            entry.format_check(parsed)
        self.assertIn(self.root / 'clients/orbit/.swift-format', command.call_args.args[0])
        self.assertIn(source / 'Orbit.swift', command.call_args.args[0])
        config.write_text('# Generated by unexpected tool\n' + body)
        with patch.object(core.Path, 'cwd', return_value=self.root), self.assertRaises(ValueError):
            core.load_profile_source(config.read_text())
        with patch.object(entry, 'run') as command, self.assertRaises(ValueError):
            entry.swift_test(self.profile)
        command.assert_not_called()

    def test_owned_loader_ignores_timestamp_aliased_bytecode(self):
        source = self.root / 'owned.py'
        source.write_text('VALUE = "evil"\n')
        py_compile.compile(str(source), doraise=True)
        stamp = source.stat().st_mtime_ns
        source.write_text('VALUE = "safe"\n')
        os.utime(source, ns=(stamp, stamp))
        loader = entry.SourceOnlyLoader('owned', str(source))
        namespace = {}
        exec(loader.get_code('owned'), namespace)
        self.assertEqual(namespace['VALUE'], 'safe')
        alias = self.root / 'alias.py'
        alias.symlink_to(source)
        with self.assertRaises(ValueError):
            entry.SourceOnlyLoader('alias', str(alias)).get_code('alias')

    def test_run_uses_argv_without_shell_and_merges_diagnostics(self):
        completed = subprocess.CompletedProcess(['tool'], 0, stdout='ok')
        with patch.object(core.subprocess, 'run', return_value=completed) as command:
            self.assertEqual(core.run(['tool', 'argument with spaces']), 'ok')
        self.assertNotIn('shell', command.call_args.kwargs)
        self.assertTrue(command.call_args.kwargs['check'])
        self.assertEqual(command.call_args.kwargs['stderr'], subprocess.STDOUT)
        self.assertEqual(command.call_args.args[0], ['tool', 'argument with spaces'])


if __name__ == '__main__':
    unittest.main()

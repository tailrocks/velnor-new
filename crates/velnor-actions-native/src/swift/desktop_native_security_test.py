"""Native source admission, archive scope, and parallel report regressions."""
import os
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import desktop_native as entry
import desktop_native_build as build
import desktop_native_core as core
import desktop_native_verify as verify
from desktop_native_test import orbit


class SecurityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name).resolve()

    def tearDown(self):
        self.temporary.cleanup()

    def git(self, root, *args):
        result = subprocess.run(['/usr/bin/git', '-C', str(root), *args], check=True,
                                env=core.subprocess_environment(), text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return result.stdout.strip()

    def repository(self, name):
        root = self.root / name
        root.mkdir()
        self.git(root, 'init', '--quiet')
        (root / 'identity').write_text(name)
        self.git(root, 'add', 'identity')
        self.git(root, '-c', 'user.name=Qualification', '-c', 'user.email=q@example.org',
                 '-c', 'commit.gpgsign=false', 'commit', '--quiet', '-m', name)
        return root, self.git(root, 'rev-parse', 'HEAD')

    def test_git_environment_cannot_redirect_source_admission(self):
        admitted, expected = self.repository('admitted')
        other, other_sha = self.repository('other')
        self.assertNotEqual(expected, other_sha)
        profile = orbit()
        profile['_root'] = other
        attack = {'GIT_DIR': str(admitted / '.git'), 'GIT_WORK_TREE': str(admitted),
                  'GIT_COMMON_DIR': str(admitted / '.git'), 'GIT_CONFIG_COUNT': '1',
                  'GIT_CONFIG_KEY_0': 'core.worktree', 'GIT_CONFIG_VALUE_0': str(admitted),
                  'GIT_OBJECT_DIRECTORY': str(admitted / '.git/objects')}
        with patch.dict(os.environ, attack):
            with self.assertRaisesRegex(ValueError, 'differs from admitted SHA'):
                core.validate_source_sha(profile, expected)
            clean = core.subprocess_environment()
            self.assertFalse(any(key.startswith('GIT_') for key in clean))
            self.assertFalse(any(key.startswith('GIT_') for key in core.subprocess_environment(trusted=True)))
            profile['_root'] = admitted
            core.validate_source_sha(profile, expected)
            nested = admitted / 'source'
            nested.mkdir()
            profile['_root'] = nested
            core.validate_source_sha(profile, expected)
            profile['_root'] = self.root / 'missing'
            with self.assertRaises(subprocess.CalledProcessError):
                core.validate_source_sha(profile, expected)

    def archive(self, extras):
        archive = self.root / 'app.zip'
        with zipfile.ZipFile(archive, 'w') as handle:
            handle.writestr('OrbitApp.app/Contents/Info.plist', b'plist')
            for name, data in extras:
                handle.writestr(name, data)
        return archive

    def test_zip_rejects_all_foreign_regular_entries(self):
        for name in ['payload.sh', 'Other.app/Contents/a', 'nested/OrbitApp.app/Contents/a',
                     'OrbitApp.app/Contents/Nested.app/Info.plist']:
            with self.assertRaises(ValueError):
                verify.archive_inventory(self.archive([(name, b'payload')]), 'OrbitApp')

    def test_appledouble_must_be_known_metadata_mirroring_app(self):
        header = b'\x00\x05\x16\x07\x00\x02\x00\x00'
        valid = [('__MACOSX/', b''), ('__MACOSX/OrbitApp.app/', b''),
                 ('__MACOSX/OrbitApp.app/Contents/._Info.plist', header + b'metadata'),
                 ('__MACOSX/._OrbitApp.app', header + b'metadata')]
        self.assertEqual(verify.archive_inventory(self.archive(valid), 'OrbitApp'), 'OrbitApp.app')
        for name, data in [('__MACOSX/payload.sh', header),
                           ('__MACOSX/Other.app/._Info.plist', header),
                           ('__MACOSX/OrbitApp.app/Contents/._missing', header),
                           ('__MACOSX/OrbitApp.app/Contents/._Info.plist', b'not AppleDouble'),
                           ('__MACOSX/foreign/', b'')]:
            with self.assertRaises(ValueError):
                verify.archive_inventory(self.archive([(name, data)]), 'OrbitApp')

    def report(self, cases, tests='1', failures='0', errors='0'):
        return ('<testsuites><testsuite tests="' + tests + '" failures="' + failures +
                '" errors="' + errors + '">' + cases + '</testsuite></testsuites>')






    def artifacts(self, kind='library'):
        profile = orbit()
        profile['_root'] = self.root
        profile['_source_sha'] = 'a' * 40
        profile['_rust_profile_digest'] = 'b3-' + 'b' * 64
        root = self.root / 'artifacts'
        bindings = root / 'bindings/BoltFFI'
        bindings.mkdir(parents=True)
        (bindings / 'boltffi.h').write_bytes(b'void orbit(void);\n')
        (bindings / 'Orbit.swift').write_text('struct Orbit {}  \n\n')
        library = root / 'library/liborbit_bridge.a'
        library.parent.mkdir()
        library.write_bytes(b'archive')
        receipt = root / 'artifacts.json'
        record = {'schema': 1, 'producer_kind': kind, 'source_sha': profile['_source_sha'],
                  'profile_digest': profile['_rust_profile_digest'], 'header_namespace': 'orbit-bridge',
                  'module_name': 'OrbitCoreFFI', 'generated_header': 'artifacts/bindings/BoltFFI/boltffi.h',
                  'bindings_path': 'artifacts/bindings', 'library_path': 'artifacts/library/liborbit_bridge.a'}
        record['hashes'] = {name: hashlib.sha256(data).hexdigest() for name, data in build.files(root).items()}
        receipt.write_text(json.dumps(record))
        profile['_rust_artifacts'] = receipt
        return profile, record

    def test_artifact_receipt_binds_source_profile_paths_and_inventory(self):
        profile, record = self.artifacts()
        artifact = build.load_artifacts(profile, library=True)
        self.assertEqual(artifact['library_path'], self.root / 'artifacts/library/liborbit_bridge.a')
        for field, value in [('source_sha', 'c' * 40), ('profile_digest', 'b3-' + 'd' * 64),
                             ('producer_kind', 'bindings'), ('generated_header', '../escape.h'),
                             ('generated_header', 'outside.h'), ('module_name', 'OtherModule')]:
            altered = dict(record, **{field: value})
            profile['_rust_artifacts'].write_text(json.dumps(altered))
            with self.assertRaises(ValueError):
                build.load_artifacts(profile, library=True)
        profile['_rust_artifacts'].write_text(json.dumps(record))
        artifact['library_path'].write_bytes(b'tampered')
        with self.assertRaisesRegex(ValueError, 'inventory or bytes'):
            build.load_artifacts(profile, library=True)

    def test_binding_normalization_is_read_only_and_detects_drift(self):
        profile, _ = self.artifacts()
        committed = core.path(profile, 'ffi.bindings_path') / 'BoltFFI'
        committed.mkdir(parents=True)
        (committed / 'Orbit.swift').write_text('struct Orbit {}\n')
        original = (self.root / 'artifacts/bindings/BoltFFI/Orbit.swift').read_bytes()
        build.bindings_check(profile)
        self.assertEqual((self.root / 'artifacts/bindings/BoltFFI/Orbit.swift').read_bytes(), original)
        (committed / 'Orbit.swift').write_text('different\n')
        with self.assertRaises(ValueError):
            build.bindings_check(profile)

    def test_owned_assembly_uses_only_verified_foreign_artifact_paths(self):
        profile, _ = self.artifacts()
        artifact = build.load_artifacts(profile, library=True)
        stage = self.root / 'stage'
        stage.mkdir()
        archive = self.root / 'build/OrbitCore.xcframework.zip'
        with patch.object(build, 'run', side_effect=['arm64', '', '']) as command:
            build.create_framework(profile, artifact, stage, archive)
        self.assertEqual((stage / 'Headers/orbit-bridge/orbit-bridge.h').read_bytes(),
                         artifact['generated_header'].read_bytes())
        self.assertEqual((stage / 'Headers/module.modulemap').read_text(),
                         'module OrbitCoreFFI {\n    header "orbit-bridge/orbit-bridge.h"\n    export *\n}\n')
        self.assertEqual(command.call_args_list[1].args[0], ['xcodebuild', '-create-xcframework',
                         '-library', artifact['library_path'], '-headers', stage / 'Headers',
                         '-output', self.root / 'build/OrbitCore.xcframework'])
        (self.root / 'build').mkdir()
        archive.symlink_to(self.root / 'missing-outside')
        with patch.object(build, 'run') as command, patch.object(build, 'load_artifacts') as load:
            with self.assertRaisesRegex(ValueError, 'symlink'):
                build.assemble_framework(profile)
        command.assert_not_called()
        load.assert_not_called()

    def test_parallel_xctest_counts_report_without_serial_console(self):
        profile = orbit()
        profile['_root'] = self.root
        profile['checks'] = {'swift_test_frameworks': ['xctest']}
        native = self.root / 'clients/orbit'
        native.mkdir(parents=True)
        xml = self.report('<testcase classname="OrbitTests" name="testOrbit"/>')
        def parallel(argv, **kwargs):
            report = Path(argv[argv.index('--xunit-output') + 1])
            report.write_text(xml)
            return '[1/1] Testing OrbitTests/testOrbit\n'
        with patch.object(entry, 'run', side_effect=parallel):
            entry.swift_test(profile)
        report = native / '.build/velnor-swift-tests.xml'
        self.assertEqual(entry.xctest_report_totals(report), 1)
        for body in [self.report('', tests='1'), self.report('', tests='0'),
                     self.report('<testcase classname="OrbitTests" name="a"><skipped/></testcase>'),
                     self.report('<testcase classname="OrbitTests" name="a"><failure/></testcase>'),
                     '<corrupt', '<testsuites/>']:
            report.write_text(body)
            with self.assertRaises((ValueError, entry.ET.ParseError)):
                entry.xctest_report_totals(report)
        report.unlink()
        with self.assertRaises(FileNotFoundError):
            entry.xctest_report_totals(report)


if __name__ == '__main__':
    unittest.main()

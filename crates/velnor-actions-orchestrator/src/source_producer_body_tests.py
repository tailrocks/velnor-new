"""Behavioral fixtures for the compiled native Cargo source owner."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

BODY = Path(__file__).with_name('source_producer_body.py')
SPEC = importlib.util.spec_from_file_location('source_producer_body', BODY)
OWNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(OWNER)
ARCHIVE = b'admitted-crate-bytes'
CHECKSUM = hashlib.sha256(ARCHIVE).hexdigest()


def admitted():
    return {'schema': 1, 'rust_version': '1.98.1', 'target': 'x86_64-unknown-linux-gnu',
            'roots': [''], 'manifests': [['Cargo.toml',
                '[package]\nname="fixture"\nversion="0.1.0"\nedition="2024"\n'
                '[dependencies]\nfixture_dep="=1.0.0"\n']],
            'locks': [['', 'version = 4\n[[package]]\nname="fixture_dep"\n'
                'version="1.0.0"\nsource="' + OWNER.REGISTRY + '"\n'
                'checksum="' + CHECKSUM + '"\n']],
            'archives': [['fixture_dep', '1.0.0', CHECKSUM]],
            'mode': 'complete-locked-workspace'}


class ProducerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name).resolve()
        self.home = self.root / 'velnor'
        self.bin = self.home / 'rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin'
        self.bin.mkdir(parents=True)
        self.log = self.root / 'calls.jsonl'
        self.output = self.root / 'output'
        self.env = patch.dict(os.environ, {'RUNNER_TEMP': str(self.root),
            'VELNOR_SOURCE_IDENTITY': 'fixture', 'GITHUB_OUTPUT': str(self.output),
            'CARGO_REGISTRY_TOKEN': 'credential-secret', 'HTTPS_PROXY': 'proxy-secret',
            'CARGO_HOME': '/bad/cargo', 'BASH_ENV': '/bad/startup',
            'RUSTC_WRAPPER': '/bad/wrapper', 'CARGO_NET_OFFLINE': 'false'}, clear=True)
        self.env.start()
        self.install_tools()

    def tearDown(self):
        self.env.stop()
        self.temp.cleanup()

    def install_tools(self, fail=False, host='x86_64-unknown-linux-gnu'):
        rustc = '#!' + sys.executable + '\nprint("rustc 1.98.1\\nrelease: 1.98.1\\nhost: ' + host + '")\n'
        (self.bin / 'rustc').write_text(rustc)
        cargo = '''import json, os, pathlib, sys
with open(LOG, 'a') as stream:
    stream.write(json.dumps({'argv': sys.argv[1:], 'env': dict(os.environ)}) + '\\n')
if sys.argv[1:] == ['-V']:
    print('cargo 1.98.1 (fixture)')
    sys.exit(0)
assert sys.argv[1] in ('fetch', 'tree')
assert '--locked' in sys.argv
cache = pathlib.Path(os.environ['CARGO_HOME']) / 'registry/cache' / IDENTITY
archive = cache / 'fixture_dep-1.0.0.crate'
if FAIL:
    print('https://credential-secret@evil.example', file=sys.stderr)
    sys.exit(1)
if '--offline' in sys.argv:
    sys.exit(0 if archive.exists() else 1)
cache.mkdir(parents=True, exist_ok=True)
archive.write_bytes(ARCHIVE)
index = pathlib.Path(os.environ['CARGO_HOME']) / 'registry/index' / IDENTITY
index.mkdir(parents=True, exist_ok=True)
(index / 'config.json').write_text(json.dumps({'dl': 'https://static.crates.io/crates', 'api': 'https://crates.io'}))
'''
        prefix = '#!' + sys.executable + '\n'
        prefix += 'LOG=' + repr(str(self.log)) + '\nIDENTITY=' + repr(OWNER.IDENTITY)
        prefix += '\nARCHIVE=' + repr(ARCHIVE) + '\nFAIL=' + repr(fail) + '\n'
        (self.bin / 'cargo').write_text(prefix + cargo)
        for name in ('cargo', 'rustc'):
            (self.bin / name).chmod(0o700)

    def run_owner(self, data=None):
        encoded = json.dumps(data or admitted()).encode().hex()
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(sys, 'argv', ['owner', encoded]):
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                result = OWNER.main()
        return result, stdout.getvalue(), stderr.getvalue()

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_native_fill_then_offline_proof_and_isolated_environment(self):
        code, stdout, stderr = self.run_owner()
        self.assertEqual((code, stderr), (0, ''))
        self.assertIn('source_scope=complete_locked_workspace', stdout)
        self.assertIn('source_downloaded_archive_bytes=unknown', stdout)
        self.assertEqual(self.output.read_text(), 'verified=true\nerror=NONE\n')
        calls = self.calls()
        self.assertEqual([call['argv'][0] for call in calls], ['-V', 'fetch', 'fetch'])
        self.assertNotIn('--offline', calls[1]['argv'])
        self.assertIn('--offline', calls[2]['argv'])
        for call in calls:
            env = call['env']
            for secret in ('CARGO_REGISTRY_TOKEN', 'HTTPS_PROXY', 'BASH_ENV',
                           'RUSTC_WRAPPER', 'CARGO_NET_OFFLINE', 'GITHUB_OUTPUT'):
                self.assertNotIn(secret, env)
            self.assertEqual(env['CARGO_HOME'], str(self.home / 'cargo'))
            self.assertEqual(env['RUSTC'], str(self.bin / 'rustc'))
        project = self.home / 'rust-source-producer/project'
        self.assertEqual((project / 'Cargo.toml').read_text(), admitted()['manifests'][0][1])
        self.assertEqual((project / 'Cargo.lock').read_text(), admitted()['locks'][0][1])
        self.assertTrue((project / 'build.rs').is_file())

    def test_corrupt_and_unqualified_archive_removed_then_native_repaired(self):
        cache = self.home / 'cargo/registry/cache' / OWNER.IDENTITY
        cache.mkdir(parents=True)
        (cache / 'fixture_dep-1.0.0.crate').write_bytes(b'corrupt')
        (cache / 'unselected-1.0.0.crate').write_bytes(b'foreign')
        self.assertEqual(self.run_owner()[0], 0)
        self.assertEqual([item.name for item in cache.iterdir()], ['fixture_dep-1.0.0.crate'])
        self.assertEqual((cache / 'fixture_dep-1.0.0.crate').read_bytes(), ARCHIVE)

    def test_warm_cache_without_receipt_requires_native_online_index_refill(self):
        self.assertEqual(self.run_owner()[0], 0)
        self.log.unlink()
        self.assertEqual(self.run_owner()[0], 0)
        self.assertNotIn('--offline', self.calls()[1]['argv'])
        self.assertIn('--offline', self.calls()[2]['argv'])

    def test_fetch_failure_static_reason_no_credentials(self):
        self.install_tools(fail=True)
        code, stdout, stderr = self.run_owner()
        self.assertEqual(code, 1)
        self.assertEqual(stderr, 'source_fetch\n')
        self.assertNotIn('credential-secret', stdout + stderr + self.output.read_text())
        self.assertEqual(self.output.read_text(), 'verified=false\nerror=source_fetch\n')

    def test_wrong_host_stops_before_fetch(self):
        self.install_tools(host='wrong-host')
        self.assertEqual(self.run_owner()[2], 'source_toolchain\n')
        self.assertEqual([call['argv'] for call in self.calls()], [['-V']])

    def test_symlink_and_hostile_registry_configuration_fail_closed(self):
        cargo_home = self.home / 'cargo'
        cargo_home.mkdir()
        (cargo_home / 'config.toml').symlink_to('/etc/passwd')
        self.assertEqual(self.run_owner()[2], 'source_path\n')
        (cargo_home / 'config.toml').unlink()
        index = cargo_home / 'registry/index' / OWNER.IDENTITY
        index.mkdir(parents=True)
        (index / 'config.json').write_text('{"dl":"https://evil.example"}')
        self.assertEqual(self.run_owner()[0], 0)
        self.assertEqual(json.loads((index / 'config.json').read_text()),
            {'dl': 'https://static.crates.io/crates', 'api': 'https://crates.io'})

    def test_target_escape_collision_and_git_lock_rejected(self):
        for path in ('../escape.rs', '/tmp/escape.rs', 'Cargo.lock', '.cargo/config.toml'):
            data = admitted()
            data['manifests'][0][1] += '[lib]\npath=' + json.dumps(path) + '\n'
            self.assertNotEqual(self.run_owner(data)[0], 0)
        data = admitted()
        data['locks'][0][1] = data['locks'][0][1].replace(OWNER.REGISTRY, 'git+https://evil.example')
        self.assertEqual(self.run_owner(data)[2], 'source_descriptor\n')
        self.assertFalse(self.log.exists())

    def test_confined_sibling_path_dependency_allowed(self):
        OWNER.confined_dependency('../sibling', Path('members/first'))
        with self.assertRaises(OWNER.Failure):
            OWNER.confined_dependency('../../escape', Path('member'))

    def test_ambient_tool_key_strings_not_consumed_or_forwarded(self):
        values = {name: 'arbitrary-untrusted-string' for name in (
            'VELNOR_RUST_SOURCE_TOOL_KEY', 'VELNOR_RUST_SOURCE_TOOL_DIGEST',
            'VELNOR_RUST_SOURCE_TOOL_IDENTITY')}
        with patch.dict(os.environ, values):
            self.assertEqual(self.run_owner()[0], 0)
        for call in self.calls():
            self.assertFalse(values.keys() & call['env'].keys())

    def test_ancestor_cargo_config_prevents_execution(self):
        config = self.home / '.cargo/config.toml'
        config.parent.mkdir()
        config.write_text('[net]\noffline=true\n')
        self.assertEqual(self.run_owner()[2], 'source_path\n')
        self.assertFalse(self.log.exists())

    def test_named_and_explicit_targets_get_empty_placeholders(self):
        data = admitted()
        data['manifests'][0][1] += ('[[bin]]\nname="other"\n'
                                  '[[test]]\nname="fixture_test"\n'
                                  '[[example]]\nname="demo"\npath="custom/demo.rs"\n'
                                  '[[bench]]\nname="speed"\n')
        self.assertEqual(self.run_owner(data)[0], 0)
        project = self.home / 'rust-source-producer/project'
        for path in ('src/bin/other.rs', 'tests/fixture_test.rs',
                     'custom/demo.rs', 'benches/speed.rs'):
            self.assertEqual((project / path).read_text(), 'fn main() {}\n')

    def test_sparse_crates_io_lock_identity_allowed(self):
        data = admitted()
        data['locks'][0][1] = data['locks'][0][1].replace(
            OWNER.REGISTRY, 'sparse+https://index.crates.io/')
        self.assertEqual(self.run_owner(data)[0], 0)

    def test_tool_descendant_symlink_prevents_any_execution(self):
        rustc = self.bin / 'rustc'
        rustc.unlink()
        rustc.symlink_to(sys.executable)
        self.assertEqual(self.run_owner()[2], 'source_path\n')
        self.assertFalse(self.log.exists())

    def test_image_qualified_key_shape_cannot_replace_toolchain_proof(self):
        self.install_tools(host='wrong-host')
        identity = 'velnor-tools3-linux-x86_64-abc-ubuntu24-20261003'
        with patch.dict(os.environ, {'VELNOR_RUST_SOURCE_TOOL_IDENTITY': identity,
                'VELNOR_RUST_SOURCE_TOOL_KEY': identity + '-snapshot-' + CHECKSUM + '-123-1'}):
            self.assertEqual(self.run_owner()[2], 'source_toolchain\n')

    def test_unexpected_restored_sparse_file_removed_before_native_fill(self):
        index = self.home / 'cargo/registry/index' / OWNER.IDENTITY
        index.mkdir(parents=True)
        (index / 'evil.bin').write_bytes(b'nonpublic-secret')
        self.assertEqual(self.run_owner()[0], 0)
        self.assertFalse((index / 'evil.bin').exists())

    def test_untrusted_canonical_sparse_records_and_extracted_sources_discarded(self):
        registry = self.home / 'cargo/registry'
        record = registry / 'index' / OWNER.IDENTITY / '.cache/fi/xt/fixture_dep'
        record.parent.mkdir(parents=True)
        record.write_bytes(b'opaque-secret')
        source = registry / 'src' / OWNER.IDENTITY / 'fixture_dep-1.0.0/Cargo.toml'
        source.parent.mkdir(parents=True)
        source.write_text('arbitrary-secret')
        code, stdout, _ = self.run_owner()
        self.assertEqual(code, 0)
        self.assertFalse(record.exists())
        self.assertFalse(source.exists())
        self.assertIn('source_untrusted_refill_elapsed_ms=', stdout)

    def test_unexpected_sparse_file_after_fill_rejected(self):
        original = OWNER.fetch
        def inject(*arguments):
            result = original(*arguments)
            if not arguments[-1]:
                index = self.home / 'cargo/registry/index' / OWNER.IDENTITY
                (index / 'evil.bin').write_bytes(b'nonpublic-secret')
            return result
        with patch.object(OWNER, 'fetch', side_effect=inject):
            self.assertEqual(self.run_owner()[2], 'source_cache\n')
        self.assertNotIn('verified=true', self.output.read_text())

    def selected(self, default=True, features=None, target=None):
        data = admitted()
        data['mode'] = 'native-tree-selected-containing'
        data['selections'] = [{'root': '', 'package': 'fixture', 'target': target,
            'features': features or [], 'default_features': default}]
        return data

    def test_selected_default_and_alpha_target_native_argv(self):
        for default, features, target in ((True, [], None),
                (False, ['alpha'], 'aarch64-unknown-linux-gnu')):
            data = self.selected(default, features, target)
            self.assertEqual(self.run_owner(data)[0], 0)
            for call in self.calls()[1:]:
                args = call['argv']
                self.assertEqual(args[0], 'tree')
                self.assertEqual(args[args.index('-p') + 1], 'fixture')
                self.assertEqual(args[args.index('-e') + 1], 'normal,build,dev')
                self.assertEqual('--no-default-features' in args, not default)
                self.assertEqual('--features' in args, bool(features))
                self.assertEqual('--target' in args, target is not None)
                if features:
                    self.assertEqual(args[args.index('--features') + 1], 'alpha')
                if target:
                    self.assertEqual(args[args.index('--target') + 1], target)
            self.log.unlink()

    def test_selected_containment_allows_unfetched_other_locked_packages(self):
        data = self.selected()
        data['locks'][0][1] += ('[[package]]\nname="unused"\nversion="1.0.0"\n'
            'source="' + OWNER.REGISTRY + '"\nchecksum="' + CHECKSUM + '"\n')
        data['archives'].append(['unused', '1.0.0', CHECKSUM])
        self.assertEqual(self.run_owner(data)[0], 0)

    def test_unknown_mode_and_empty_selections_rejected_without_execution(self):
        data = admitted()
        data['mode'] = 'unknown'
        self.assertEqual(self.run_owner(data)[2], 'source_descriptor\n')
        data = self.selected()
        data['selections'] = []
        self.assertEqual(self.run_owner(data)[2], 'source_descriptor\n')
        self.assertFalse(self.log.exists())


if __name__ == '__main__':
    unittest.main()

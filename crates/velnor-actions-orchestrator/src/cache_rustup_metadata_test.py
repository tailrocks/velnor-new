"""Adversarial neutral bridge tests; fixture seals are not attestation proof."""
import sys
import copy
import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"))
import cache_receipt
import cache_rustup_metadata as bridge
from cache_receipt_common import ColdReceipt
from cache_receipt_test import layout, policy as base_policy, replace as owner_fixture_replace


def descriptor():
    chain = '1.98.1-x86_64-unknown-linux-gnu'
    return {'schema': 1, 'role': 'root-linux', 'host': 'x86_64-unknown-linux-gnu',
            'manager_version': '1.29.1',
            'manager_sha256': 'dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71',
            'qualification': dict(bridge._QUALIFICATION), 'toolchain': chain,
            'manager': 'cargo/bin/rustup', 'proxy': 'cargo/bin/cargo',
            'settings': 'rustup/settings.toml',
            'cargo': 'rustup/toolchains/' + chain + '/bin/cargo',
            'rustc': 'rustup/toolchains/' + chain + '/bin/rustc',
            'rustdoc': 'rustup/toolchains/' + chain + '/bin/rustdoc'}


def policy_fixture():
    optional = bridge._FULL_ROOTS[-2:]
    return owner_fixture_replace(base_policy(), role='tool-full', allowed_roots=bridge._FULL_ROOTS,
                                 optional_roots=optional,
                                 transport_layout=layout(bridge._FULL_ROOTS, optional))


def policy_digest(policy):
    data = json.dumps(policy.source_record(), sort_keys=True, separators=(',', ':'),
                      ensure_ascii=True) + '\n'
    return hashlib.sha256(data.encode('ascii')).hexdigest()


class BridgeTests(unittest.TestCase):
    def setUp(self):
        self.policy = SimpleNamespace(role='tool-full', allowed_roots=bridge._FULL_ROOTS,
                                      descriptor_sha256='d' * 64)
        self.cwd = os.path.realpath(tempfile.gettempdir())
        self.descriptor = descriptor()
        self.entries = [{'path': self.descriptor[field], 'kind': 'file', 'mode': 0o755,
                         'sha256': self.descriptor['manager_sha256']
                         if field in ('manager', 'proxy') else 'a' * 64}
                        for field in ('manager', 'proxy', 'settings', 'cargo', 'rustc', 'rustdoc')]
        self.manifest = json.dumps({'schema': 2, 'entries': self.entries}).encode()

    def test_public_constructor_and_forged_or_copied_grants_reject(self):
        for value in ({}, SimpleNamespace(root='/tmp/velnor'), object()):
            with self.assertRaises(ColdReceipt):
                bridge.VerifiedRustupMetadataLaunch(value, {}, {}, '/tmp')
        forged = object.__new__(cache_receipt.VerifiedFullToolPayload)
        object.__setattr__(forged, '_seal', object())
        with self.assertRaises(ColdReceipt):
            forged.require_current()
        with self.assertRaises(ColdReceipt):
            copy.copy(forged)

    def test_unqualified_and_modified_descriptor_reject_before_manager(self):
        values = [None]
        for field in ('schema', 'host', 'manager_sha256', 'manager', 'settings', 'cargo', 'rustc'):
            value = descriptor()
            value[field] = 'foreign'
            values.append(value)
        for value in values:
            with patch.object(bridge, '_COMPILED_RUSTUP_METADATA', value), \
                    patch.object(bridge, '_which') as child:
                with self.assertRaises(ColdReceipt):
                    bridge.verify_rustup_metadata_launch('/tmp', None, None, self.policy, {}, '/tmp')
                child.assert_not_called()

    def test_plain_verifier_result_cannot_grant_manager_execution(self):
        policy = policy_fixture()
        with patch.object(bridge, '_COMPILED_RUSTUP_METADATA', self.descriptor), \
                patch.object(bridge, '_COMPILED_RECEIPT_POLICY_SHA256', policy_digest(policy)), \
                patch.object(cache_receipt, 'verify_live_payload', return_value={}), \
                patch.object(bridge, '_which') as child:
            with self.assertRaises(ColdReceipt):
                bridge.verify_rustup_metadata_launch('/tmp', None, None, policy, {}, self.cwd)
            child.assert_not_called()

    def test_caller_selected_policy_and_qualification_booleans_cannot_grant(self):
        policy = policy_fixture()
        with patch.object(bridge, '_COMPILED_RECEIPT_POLICY_SHA256', policy_digest(policy)):
            bridge._receipt_policy(policy)
            for field, value in (('signer_uri', 'foreign'), ('descriptor_sha256', 'f' * 64),
                                 ('public_attestation_qualified', False), ('source_sha', 'f' * 40)):
                with self.assertRaises(ColdReceipt):
                    bridge._receipt_policy(owner_fixture_replace(policy, **{field: value}))

    def test_wrong_scope_rejects_before_verification_and_manager(self):
        policy = SimpleNamespace(role='tool-planning', allowed_roots=bridge._FULL_ROOTS)
        with patch.object(bridge, '_COMPILED_RUSTUP_METADATA', self.descriptor), \
                patch.object(cache_receipt, 'verify_live_payload') as verify, \
                patch.object(bridge, '_which') as child:
            with self.assertRaises(ColdReceipt):
                bridge.verify_rustup_metadata_launch('/tmp', None, None, policy, {}, '/tmp')
            verify.assert_not_called()
            child.assert_not_called()

    def test_missing_settings_or_digest_mismatch_rejects(self):
        for entries in (self.entries[0:2] + self.entries[3:],
                        [dict(entry, sha256='f' * 64) for entry in self.entries]):
            manifest = json.dumps({'schema': 2, 'entries': entries}).encode()
            with self.assertRaises(ColdReceipt):
                bridge._inventory(manifest, self.descriptor)

    def test_fixture_live_seal_binds_settings_and_full_manifest(self):
        # Private fixture seam only; does not qualify any source/attestation.
        grant = cache_receipt.VerifiedFullToolPayload(
            cache_receipt._FULL_AUTHORITY, '/tmp/velnor', self.manifest, self.policy)
        with patch.object(cache_receipt, 'inventory_exact_roots', return_value=self.manifest + b' '):
            with self.assertRaises(ColdReceipt):
                grant.require_current()

    def test_fixture_launch_preserves_exact_proxy_selector_environment_cwd(self):
        grant = cache_receipt.VerifiedFullToolPayload(
            cache_receipt._FULL_AUTHORITY, '/tmp/velnor', self.manifest, self.policy)
        environment = {'CARGO_HOME': '/tmp/velnor/cargo', 'RUSTUP_HOME': '/tmp/velnor/rustup',
                       'RUSTUP_AUTO_INSTALL': '0', 'FEATURE': 'exact'}
        with patch.object(cache_receipt, 'inventory_exact_roots', return_value=self.manifest):
            launch = bridge.VerifiedRustupMetadataLaunch(
                grant, self.descriptor, environment, self.cwd, _seal=bridge._LAUNCH_SEAL)
            arguments = ['metadata', '--format-version', '1', '--locked', '--offline']
            command, actual_environment, cwd = launch.command(arguments)
            self.assertEqual(command, ['/tmp/velnor/cargo/bin/cargo',
                                      '+1.98.1-x86_64-unknown-linux-gnu', *arguments])
            self.assertEqual(actual_environment, environment)
            self.assertEqual(cwd, self.cwd)
            self.assertEqual(launch.installed_tool('rustc'),
                             ('/tmp/velnor/' + self.descriptor['rustc'], 'a' * 64))
            with patch.object(bridge, '_observe_child', return_value=(b'{}', 0, 123)) as observe:
                self.assertEqual(launch.observe_metadata(arguments, original_program=command[0],
                                  original_arguments=[command[1], 'test']), (b'{}', 0, 123))
                observe.assert_called_once_with(command, environment, self.cwd,
                                                16 * 1024 * 1024, 30)
            with patch.object(bridge, '_observe_child') as observe, \
                    patch.object(bridge, '_which') as which:
                for program, original in (('/foreign/cargo', [command[1], 'test']),
                                           (command[0], ['+stable', 'test']),
                                           (command[0], [b'test'])):
                    with self.assertRaises(ColdReceipt):
                        launch.observe_metadata(arguments, original_program=program,
                                                original_arguments=original)
                observe.assert_not_called()
                which.assert_not_called()
            with patch.object(bridge, '_observe_child', return_value=(b'{}', 0, 123)), \
                    patch.object(bridge, '_which', return_value=b'/foreign/cargo\n'):
                with self.assertRaises(ColdReceipt):
                    launch.observe_metadata(arguments, original_program=command[0],
                                            original_arguments=['test'])
            with self.assertRaises(ColdReceipt):
                launch._cwd = '/foreign'

    def test_exact_environment_and_roots(self):
        environment = {'CARGO_HOME': '/tmp/velnor/cargo', 'RUSTUP_HOME': '/tmp/velnor/rustup',
                       'FEATURE': 'exact', 'RUSTUP_AUTO_INSTALL': '1'}
        actual = bridge._environment(environment, '/tmp/velnor')
        self.assertEqual(actual, dict(environment, RUSTUP_AUTO_INSTALL='0'))
        self.assertEqual(environment['RUSTUP_AUTO_INSTALL'], '1')
        for mutation in ({'LD_PRELOAD': '/evil'}, {'CARGO_HOME': '/foreign'}):
            with self.assertRaises(ColdReceipt):
                bridge._environment(dict(environment, **mutation), '/tmp/velnor')

    def test_metadata_only_arguments_preserve_selector_owned_prefix(self):
        arguments = ['--config', 'build.target-dir="out"', 'metadata',
                     '--format-version', '1', '--locked', '--offline', '--features=one']
        self.assertEqual(bridge._metadata_arguments(arguments), arguments)
        for invalid in (['build', *arguments], ['+stable', *arguments],
                        [*arguments, '--no-deps'],
                        ['metadata', '--format-version', '1'],
                        ['metadata', '--format-version', '2', '--locked', '--offline']):
            with self.assertRaises(ColdReceipt):
                bridge._metadata_arguments(invalid)

    def test_which_bounds_output_and_timeout(self):
        with tempfile.TemporaryDirectory() as root:
            output = bridge._which(['/usr/bin/printf', '/actual/cargo\n'], {}, root)
            self.assertEqual(output, b'/actual/cargo\n')
            with self.assertRaises(ColdReceipt):
                bridge._which(['/usr/bin/python3', '-I', '-S', '-c',
                               'print("x" * 4097)'], {}, str(Path(root)))

    def test_observer_records_actual_child_wait_wall_and_stdout(self):
        output, status, wall = bridge._observe_child(
            ['/usr/bin/python3', '-I', '-S', '-c', 'print("{}"); raise SystemExit(7)'],
            {}, self.cwd, 1024, 2)
        self.assertEqual(output, b'{}\n')
        self.assertEqual(status, 7)
        self.assertGreater(wall, 0)
        with self.assertRaises(ColdReceipt):
            bridge._observe_child(['/usr/bin/python3', '-I', '-S', '-c',
                                   'import time; time.sleep(10)'], {}, self.cwd, 1024, 0.01)


if __name__ == '__main__':
    unittest.main()

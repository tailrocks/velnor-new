"""Strict compiled-helper environment transport regression tests."""
import base64
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch
SRC = Path(__file__).resolve().parents[1] / 'src'
sys.path.insert(0, str(SRC))
LAUNCHER = importlib.import_module('source_helper_launcher')
try:
    TRANSPORT = importlib.import_module('source_helper_transport')
except ModuleNotFoundError as error:
    if error.name != 'source_helper_transport':
        raise
    TRANSPORT = LAUNCHER
PREFIX = 'VELNOR_COMPILED_HELPER_'
CHUNK_SIZE = 8192
MAX_SOURCE_SIZE = 256 * 1024
MAX_ARGUMENTS_SIZE = 512 * 1024
MAX_ARGUMENTS_JSON_SIZE = 1024 * 1024
MACOS_ARG_MAX = 1024 * 1024
REJECTION = (ValueError, TypeError, SystemExit)
def json_bytes(arguments):
    """Match the compact UTF-8 JSON bytes owned by the Rust serializer."""
    return json.dumps(arguments, ensure_ascii=False, separators=(',', ':')).encode('utf-8')
def chunks(payload):
    encoded = base64.b64encode(payload).decode('ascii')
    return [encoded[offset:offset + CHUNK_SIZE]
            for offset in range(0, len(encoded), CHUNK_SIZE)]
def envelope(source, arguments_json, execution_json):
    source_chunks = chunks(source)
    argument_chunks = chunks(arguments_json)
    execution_chunks = chunks(execution_json)
    environment = {
        PREFIX + 'SCHEMA': '1',
        PREFIX + 'SOURCE_COUNT': str(len(source_chunks)),
        PREFIX + 'ARGUMENTS_COUNT': str(len(argument_chunks)),
        PREFIX + 'EXECUTION_COUNT': str(len(execution_chunks)),
    }
    environment.update({
        PREFIX + f'SOURCE_{index:04d}': chunk
        for index, chunk in enumerate(source_chunks)
    })
    environment.update({
        PREFIX + f'ARGUMENTS_{index:04d}': chunk
        for index, chunk in enumerate(argument_chunks)
    })
    environment.update({
        PREFIX + f'EXECUTION_{index:04d}': chunk
        for index, chunk in enumerate(execution_chunks)
    })
    return environment
def fixture(source, arguments, execution_prefix=()):
    source_bytes = source if isinstance(source, bytes) else source.encode('utf-8')
    arguments_json = arguments if isinstance(arguments, bytes) else json_bytes(arguments)
    execution_json = (execution_prefix if isinstance(execution_prefix, bytes)
                      else json_bytes(list(execution_prefix)))
    return (
        envelope(source_bytes, arguments_json, execution_json),
        hashlib.sha256(source_bytes).hexdigest(),
        len(source_bytes),
        hashlib.sha256(arguments_json).hexdigest(),
        len(arguments_json),
        hashlib.sha256(execution_json).hexdigest(),
        len(execution_json),
    )
def decode(environment, source_digest, source_size, arguments_digest, arguments_size,
           execution_digest, execution_size, bindings=()):
    return TRANSPORT.decode_transport(
        environment,
        source_digest,
        source_size,
        arguments_digest,
        arguments_size,
        execution_digest,
        execution_size,
        list(bindings),
    )
def with_environment(data, environment):
    return (environment, *data[1:])
class TransportTests(unittest.TestCase):
    def assert_rejected(self, environment, source_digest, source_size,
                        arguments_digest, arguments_size, execution_digest,
                        execution_size, bindings=()):
        with self.assertRaises(REJECTION):
            decode(environment, source_digest, source_size,
                   arguments_digest, arguments_size, execution_digest,
                   execution_size, bindings)
    def test_chunks_are_order_independent_and_round_trip_exact_utf8(self):
        source = ('#!/bin/bash\n# generated: source-bound helper\n'
                  + 'printf \'%s\' "é́"\n' * 5000).encode('utf-8')
        arguments = ['--literal', 'a b; $(touch should-not-run)', 'é́', '*']
        data = fixture(source, arguments)
        shuffled = {key: data[0][key] for key in reversed(list(data[0]))}
        actual_source, actual_arguments, actual_prefix = decode(shuffled, *data[1:])

        self.assertEqual(actual_source, source.decode('utf-8'))
        self.assertEqual(actual_arguments, arguments)
        self.assertEqual(actual_prefix, [])
        self.assertGreater(int(data[0][PREFIX + 'SOURCE_COUNT']), 1)
        self.assertTrue(all(
            len(value) <= CHUNK_SIZE
            for key, value in data[0].items()
            if key.startswith(PREFIX + 'SOURCE_') or key.startswith(PREFIX + 'ARGUMENTS_')
        ))

    def test_missing_and_extra_source_indices_fail_closed(self):
        data = fixture(b'#!/bin/bash\n' + b'x' * 20000, [])
        source_count = int(data[0][PREFIX + 'SOURCE_COUNT'])
        self.assertGreater(source_count, 1)
        missing = dict(data[0])
        del missing[PREFIX + 'SOURCE_0001']
        self.assert_rejected(*with_environment(data, missing))

        extra = dict(data[0])
        extra[PREFIX + f'SOURCE_{source_count:04d}'] = data[0][PREFIX + 'SOURCE_0000']
        self.assert_rejected(*with_environment(data, extra))

    def test_missing_and_extra_argument_indices_fail_closed(self):
        arguments = ['x' * 10000]
        data = fixture('#!/bin/bash\n', arguments)
        argument_count = int(data[0][PREFIX + 'ARGUMENTS_COUNT'])
        self.assertGreater(argument_count, 1)
        missing = dict(data[0])
        del missing[PREFIX + 'ARGUMENTS_0001']
        self.assert_rejected(*with_environment(data, missing))

        extra = dict(data[0])
        extra[PREFIX + f'ARGUMENTS_{argument_count:04d}'] = data[0][PREFIX + 'ARGUMENTS_0000']
        self.assert_rejected(*with_environment(data, extra))

    def test_missing_and_extra_execution_indices_fail_closed(self):
        execution_prefix = ['env', 'x' * 10000, '--']
        data = fixture('#!/bin/bash\n', [], execution_prefix)
        execution_count = int(data[0][PREFIX + 'EXECUTION_COUNT'])
        self.assertGreater(execution_count, 1)
        missing = dict(data[0])
        del missing[PREFIX + 'EXECUTION_0001']
        self.assert_rejected(*with_environment(data, missing))

        extra = dict(data[0])
        extra[PREFIX + f'EXECUTION_{execution_count:04d}'] = (
            data[0][PREFIX + 'EXECUTION_0000']
        )
        self.assert_rejected(*with_environment(data, extra))

    def test_size_and_digest_bindings_reject_mutation(self):
        data = fixture('#!/bin/bash\nprintf ok\n', ['--config', 'value'])
        environment = data[0]
        source_digest, source_size, arguments_digest, arguments_size = data[1:5]
        execution_digest, execution_size = data[5:]
        for bad_source_size, bad_arguments_size in (
            (source_size + 1, arguments_size),
            (source_size, arguments_size + 1),
        ):
            with self.subTest(source_size=bad_source_size, arguments_size=bad_arguments_size):
                self.assert_rejected(environment, source_digest, bad_source_size,
                                      arguments_digest, bad_arguments_size,
                                      execution_digest, execution_size)
        self.assert_rejected(environment, '0' * 64, source_size,
                              arguments_digest, arguments_size, execution_digest,
                              execution_size)
        self.assert_rejected(environment, source_digest, source_size,
                              '0' * 64, arguments_size, execution_digest,
                              execution_size)

        tampered = dict(environment)
        chunk_key = PREFIX + 'SOURCE_0000'
        chunk = tampered[chunk_key]
        tampered[chunk_key] = ('A' if chunk[0] != 'A' else 'B') + chunk[1:]
        self.assert_rejected(tampered, source_digest, source_size,
                              arguments_digest, arguments_size, execution_digest,
                              execution_size)
        prefix_fixture = fixture('#!/bin/bash\n', [], ['env', '--'])
        prefix_environment = prefix_fixture[0]
        prefix_source_digest, prefix_source_size = prefix_fixture[1:3]
        prefix_arguments_digest, prefix_arguments_size = prefix_fixture[3:5]
        prefix_execution_digest, prefix_execution_size = prefix_fixture[5:]
        self.assert_rejected(
            prefix_environment,
            prefix_source_digest,
            prefix_source_size,
            prefix_arguments_digest,
            prefix_arguments_size,
            '0' * 64,
            prefix_execution_size,
        )
        self.assert_rejected(
            prefix_environment,
            prefix_source_digest,
            prefix_source_size,
            prefix_arguments_digest,
            prefix_arguments_size,
            prefix_execution_digest,
            prefix_execution_size + 1,
        )
        prefix_tampered = dict(prefix_environment)
        prefix_key = PREFIX + 'EXECUTION_0000'
        prefix_chunk = prefix_tampered[prefix_key]
        prefix_tampered[prefix_key] = ('A' if prefix_chunk[0] != 'A' else 'B') + prefix_chunk[1:]
        self.assert_rejected(
            prefix_tampered,
            prefix_source_digest,
            prefix_source_size,
            prefix_arguments_digest,
            prefix_arguments_size,
            prefix_execution_digest,
            prefix_execution_size,
        )
    def test_base64_and_utf8_are_strict(self):
        data = fixture('#!/bin/bash\n', ['ok'])
        environment = data[0]
        source_digest, source_size, arguments_digest, arguments_size = data[1:5]
        execution_digest, execution_size = data[5:]
        invalid_base64 = dict(environment)
        invalid_base64[PREFIX + 'SOURCE_0000'] = '!' * len(
            invalid_base64[PREFIX + 'SOURCE_0000']
        )
        self.assert_rejected(invalid_base64, source_digest, source_size,
                              arguments_digest, arguments_size, execution_digest,
                              execution_size)

        bad_source = fixture(b'\xff', b'["ok"]')
        self.assert_rejected(*bad_source)
        bad_argument_utf8 = fixture(b'#!/bin/bash\n', b'[\xff]')
        self.assert_rejected(*bad_argument_utf8)
        bad_arguments = fixture(b'#!/bin/bash\n', b'["ok",1]')
        self.assert_rejected(*bad_arguments)

    def test_argument_values_must_be_strings(self):
        for payload in (b'["ok",1]', b'["ok",null]', b'{"arg":"ok"}',
                        b'"ok"', b'["\\ud800"]'):
            data = fixture(b'#!/bin/bash\n', payload)
            arguments_digest = hashlib.sha256(payload).hexdigest()
            self.assert_rejected(data[0], data[1], data[2], arguments_digest,
                                  len(payload), data[5], data[6])

    def test_execution_prefix_resolves_runner_temp_and_args_keep_separate_syntax(self):
        execution_prefix = ['env', '$RUNNER_TEMP/bin', '${RUNNER_TEMP}/mise', '--']
        arguments = ['${{ runner.temp }}/input', '$RUNNER_TEMP/literal']
        data = fixture('#!/bin/bash\n', arguments, execution_prefix)
        environment = dict(data[0])
        environment['RUNNER_TEMP'] = '/runner/tmp'

        decoded_source, decoded_arguments, decoded_prefix = decode(environment, *data[1:])

        self.assertEqual(decoded_source, '#!/bin/bash\n')
        self.assertEqual(decoded_arguments, ['/runner/tmp/input', '$RUNNER_TEMP/literal'])
        self.assertEqual(
            decoded_prefix,
            ['env', '/runner/tmp/bin', '/runner/tmp/mise', '--'],
        )

    def test_execution_prefix_only_resolves_declared_owner_bindings(self):
        data = fixture(b'#!/bin/bash\n', [], ['env', '${OWNER_HOME}', '--'])
        environment = dict(data[0], OWNER_HOME='/owned/home', RUNNER_TEMP='/runner/tmp')
        decoded_prefix = decode(environment, *data[1:], ['OWNER_HOME'])[2]
        self.assertEqual(decoded_prefix, ['env', '/owned/home', '--'])
        self.assert_rejected(*with_environment(data, environment), bindings=['OTHER_HOME'])

    def test_execution_prefix_requires_string_array_without_controls(self):
        for payload in (b'{"prefix":"env"}', b'["env",1]', b'["bad\\n"]',
                        b'["\\ud800"]', b'["env"]', b'["$UNSUPPORTED","--"]'):
            fixture_values = fixture(b'#!/bin/bash\n', [], payload)
            self.assert_rejected(*fixture_values)

    def test_schema_and_namespace_are_closed(self):
        data = fixture('#!/bin/bash\n', [])
        environment = data[0]
        for key, value in ((PREFIX + 'SCHEMA', '2'),
                           (PREFIX + 'UNEXPECTED', 'payload'),
                           (PREFIX + 'EXECUTION_EXTRA', 'payload')):
            altered = dict(environment)
            altered[key] = value
            self.assert_rejected(*with_environment(data, altered))
        legacy = {
            key: value for key, value in environment.items()
            if not key.startswith(PREFIX + 'EXECUTION_')
        }
        self.assert_rejected(*with_environment(data, legacy))

    def test_exact_source_and_argument_limits_are_accepted(self):
        source_prefix = b'#!/bin/bash\n'
        source = source_prefix + b'x' * (MAX_SOURCE_SIZE - len(source_prefix))
        source_fixture = fixture(source, [])
        self.assertEqual(decode(*source_fixture)[0], source.decode('utf-8'))

        arguments = ['x' * (MAX_ARGUMENTS_SIZE - 4)]
        arguments_fixture = fixture(b'#!/bin/bash\n', arguments)
        self.assertEqual(decode(*arguments_fixture)[1], arguments)

    def test_oversize_payloads_fail_explicitly_without_truncation(self):
        source = b'#!/bin/bash\n' + b'x' * (MAX_SOURCE_SIZE + 1 - len(b'#!/bin/bash\n'))
        source_fixture = fixture(source, [])
        self.assert_rejected(*source_fixture)

        arguments = ['x' * (MAX_ARGUMENTS_SIZE + 1)]
        arguments_fixture = fixture(b'#!/bin/bash\n', arguments)
        self.assertLessEqual(arguments_fixture[4], MAX_ARGUMENTS_JSON_SIZE)
        self.assert_rejected(*arguments_fixture)

        oversized_json = b' ' * (MAX_ARGUMENTS_JSON_SIZE + 1) + b'[]'
        oversized_json_fixture = fixture(b'#!/bin/bash\n', oversized_json)
        self.assertGreater(oversized_json_fixture[4], MAX_ARGUMENTS_JSON_SIZE)
        self.assert_rejected(*oversized_json_fixture)

    def test_realistic_tofu_lock_and_npm_descriptor_payload_fits_macos_bound(self):
        lock_hcl = (
            'provider "registry.opentofu.org/hashicorp/aws" {\n'
            '  version = "5.0.0"\n'
            '  hashes = [\n'
            + ''.join(f'    "zh:{index:064x}",\n' for index in range(180))
            + '  ]\n}\n'
        )
        lock_hcl += ' ' * (16 * 1024 - len(lock_hcl))
        self.assertEqual(len(lock_hcl), 16 * 1024)
        lock_octal = ''.join(f'\\{byte:03o}' for byte in lock_hcl.encode())
        integrity = 'sha512-' + 'A' * 86 + '=='
        descriptors = [
            {
                'name': f'npm-package-{index}',
                'version': '1.0.0',
                'resolved': (
                    f'https://registry.npmjs.org/npm-package-{index}/'
                    f'-npm-package-{index}-1.0.0.tgz'
                ),
                'integrity': integrity,
            }
            for index in range(1024)
        ]
        descriptor_arguments = [
            json.dumps(descriptor, separators=(',', ':'))
            for descriptor in descriptors
        ]
        source = (
            '#!/bin/bash\n'
            'printf "%s" source-bound >/dev/null\n'
        ).encode('utf-8')
        arguments = [
            '--tofu-lock-hcl-octal',
            lock_octal,
            '--native-npm-source',
            *descriptor_arguments,
        ]
        data = fixture(source, arguments)
        environment = data[0]
        source_size, arguments_size = data[2], data[4]
        source_transport_bytes = sum(
            len(key.encode('ascii')) + len(value.encode('ascii')) + 2
            for key, value in environment.items()
            if key.startswith(PREFIX + 'SOURCE_')
        )

        self.assertEqual(len(lock_hcl), 16 * 1024)
        self.assertGreater(len(lock_octal), len(lock_hcl))
        self.assertLessEqual(source_size, MAX_SOURCE_SIZE)
        argument_transport_bytes = sum(
            len(key.encode('ascii')) + len(value.encode('ascii')) + 2
            for key, value in environment.items()
            if key.startswith(PREFIX + 'ARGUMENTS_')
        )
        pointers = 8 * (len(environment) + len(arguments) + 16)
        self.assertLessEqual(
            source_transport_bytes + argument_transport_bytes + pointers + 65_536,
            MACOS_ARG_MAX,
        )
        self.assertLessEqual(arguments_size, MAX_ARGUMENTS_SIZE)
        decoded_source, decoded_arguments, decoded_prefix = decode(environment, *data[1:])
        self.assertEqual(decoded_source, source.decode('utf-8'))
        self.assertEqual(decoded_arguments, arguments)
        self.assertEqual(decoded_prefix, [])

    def test_child_environment_cannot_observe_transport_payload(self):
        transport_values = {
            PREFIX + 'SCHEMA': '1',
            PREFIX + 'SOURCE_COUNT': '1',
            PREFIX + 'SOURCE_0000': 'c2VjcmV0',
            PREFIX + 'ARGUMENTS_COUNT': '1',
            PREFIX + 'ARGUMENTS_0000': 'WyJzZWNyZXQiXQ==',
            PREFIX + 'EXECUTION_COUNT': '1',
            PREFIX + 'EXECUTION_0000': 'WyJlbnYiLC0tIl0=',
        }
        with patch.dict(os.environ, transport_values):
            child_environment = LAUNCHER.execution_environment()
        leaked = [key for key in child_environment if key.startswith(PREFIX)]
        self.assertEqual(leaked, [])
        result = subprocess.run(
            ['/bin/bash', '-c', 'test -z "${VELNOR_COMPILED_HELPER_SOURCE_COUNT+x}"'],
            env=child_environment,
            check=False,
        )
        self.assertEqual(result.returncode, 0)

if __name__ == '__main__':
    unittest.main()

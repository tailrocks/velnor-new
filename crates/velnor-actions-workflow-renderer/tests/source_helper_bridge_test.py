"""Exercise the fixed env bridge between transport decoding and bash."""
import base64
import hashlib
import importlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SRC = Path(__file__).resolve().parents[1] / 'src'
sys.path.insert(0, str(SRC))
TRANSPORT = importlib.import_module('source_helper_transport')
LAUNCHER = importlib.import_module('source_helper_launcher')
PREFIX = 'VELNOR_COMPILED_HELPER_'
CHUNK_SIZE = 8192


def chunks(payload):
    encoded = base64.b64encode(payload).decode('ascii')
    return [encoded[offset:offset + CHUNK_SIZE]
            for offset in range(0, len(encoded), CHUNK_SIZE)]


def fixture(source, arguments, execution_prefix):
    source_bytes = source.encode('utf-8')
    arguments_bytes = json.dumps(arguments, separators=(',', ':')).encode('utf-8')
    execution_bytes = json.dumps(execution_prefix, separators=(',', ':')).encode('utf-8')
    environment = {
        PREFIX + 'SCHEMA': '1',
    }
    for name, payload in (
        ('SOURCE', source_bytes),
        ('ARGUMENTS', arguments_bytes),
        ('EXECUTION', execution_bytes),
    ):
        encoded = chunks(payload)
        environment[PREFIX + name + '_COUNT'] = str(len(encoded))
        environment.update({
            PREFIX + f'{name}_{index:04d}': value
            for index, value in enumerate(encoded)
        })
    return (
        environment,
        hashlib.sha256(source_bytes).hexdigest(),
        len(source_bytes),
        hashlib.sha256(arguments_bytes).hexdigest(),
        len(arguments_bytes),
        hashlib.sha256(execution_bytes).hexdigest(),
        len(execution_bytes),
    )


class SourceHelperBridgeTests(unittest.TestCase):
    def test_launcher_overrides_ambient_rustup_auto_install(self):
        source = 'set -eu\ntest "$RUSTUP_AUTO_INSTALL" = 0\n'
        data = fixture(source, [], [])
        with tempfile.TemporaryDirectory(dir=os.path.realpath(tempfile.gettempdir())) as root:
            environment = dict(data[0], RUNNER_TEMP=root)
            for ambient in ('1', ''):
                with self.subTest(ambient=ambient), patch.dict(
                    os.environ,
                    {'RUNNER_TEMP': root, 'RUSTUP_AUTO_INSTALL': ambient},
                ):
                    decoded_source, arguments, prefix = TRANSPORT.decode_transport(
                        environment, *data[1:], []
                    )
                    self.assertEqual(prefix, [])
                    self.assertEqual(
                        LAUNCHER.execute(decoded_source, data[1], arguments, prefix), 0
                    )

            prefixed = fixture(
                source,
                [],
                ['/usr/bin/env', '-i', 'RUSTUP_AUTO_INSTALL=0',
                 '/usr/bin/env', '--'],
            )
            prefixed_environment = dict(prefixed[0], RUNNER_TEMP=root)
            with patch.dict(
                os.environ,
                {'RUNNER_TEMP': root, 'RUSTUP_AUTO_INSTALL': '1'},
            ):
                decoded_source, arguments, prefix = TRANSPORT.decode_transport(
                    prefixed_environment, *prefixed[1:], []
                )
                self.assertEqual(prefix[2], 'RUSTUP_AUTO_INSTALL=0')
                self.assertEqual(
                    LAUNCHER.execute(decoded_source, prefixed[1], arguments, prefix), 0
                )

    def test_env_i_bridge_preserves_admitted_data_and_drops_ambient_values(self):
        source = (
            "set -eu\n"
            "test -z \"${FAKE_AMBIENT+x}\"\n"
            "test -z \"${GH_TOKEN+x}\"\n"
            "printf '%s\\n' \"$ADMITTED_VALUE\" > \"$RUNNER_TEMP/result\"\n"
        )
        raw_prefix = [
            '/usr/bin/env',
            '-i',
            'RUNNER_TEMP=${RUNNER_TEMP}',
            'ADMITTED_VALUE=${ADMITTED_VALUE}',
            'PATH=${PATH}',
            '/usr/bin/env',
            '--',
        ]
        data = fixture(source, [], raw_prefix)
        with tempfile.TemporaryDirectory(dir=os.path.realpath(tempfile.gettempdir())) as root:
            environment = dict(
                data[0],
                RUNNER_TEMP=root,
                ADMITTED_VALUE='literal$VALUE',
                PATH='/usr/bin:/bin',
            )
            with patch.dict(
                os.environ,
                {
                    'RUNNER_TEMP': root,
                    'FAKE_AMBIENT': 'must-disappear',
                    'GH_TOKEN': 'must-disappear',
                },
            ):
                decoded_source, arguments, prefix = TRANSPORT.decode_transport(
                    environment, *data[1:], ['RUNNER_TEMP', 'ADMITTED_VALUE', 'PATH']
                )
                self.assertEqual(prefix, [
                    '/usr/bin/env', '-i', f'RUNNER_TEMP={root}',
                    'ADMITTED_VALUE=literal$VALUE', 'PATH=/usr/bin:/bin',
                    '/usr/bin/env', '--',
                ])
                result = LAUNCHER.execute(
                    decoded_source, data[1], arguments, prefix
                )
            self.assertEqual(result, 0)
            self.assertEqual(Path(root, 'result').read_text(), 'literal$VALUE\n')

    def test_bridge_rejects_undeclared_prefix_environment_reference(self):
        data = fixture(
            '#!/bin/bash\n', [],
            ['/usr/bin/env', '-i', 'ADMITTED_VALUE=${ADMITTED_VALUE}',
             '/usr/bin/env', '--'],
        )
        with tempfile.TemporaryDirectory(dir=os.path.realpath(tempfile.gettempdir())) as root:
            environment = dict(data[0], RUNNER_TEMP=root, ADMITTED_VALUE='owned')
            with self.assertRaises(SystemExit):
                TRANSPORT.decode_transport(
                    environment, *data[1:], ['RUNNER_TEMP']
                )

    def test_output_channel_is_explicit_and_other_command_files_are_dropped(self):
        source = (
            "set -eu\n"
            "test -z \"${GITHUB_ENV+x}\"\n"
            "test -z \"${GITHUB_PATH+x}\"\n"
            "test -z \"${GITHUB_STATE+x}\"\n"
            "test -z \"${GITHUB_STEP_SUMMARY+x}\"\n"
            "printf 'result=ok\\n' >> \"$GITHUB_OUTPUT\"\n"
        )
        raw_prefix = [
            '/usr/bin/env', '-i',
            'GITHUB_OUTPUT=${GITHUB_OUTPUT}',
            'RUNNER_TEMP=${RUNNER_TEMP}',
            '/usr/bin/env', '--',
        ]
        data = fixture(source, [], raw_prefix)
        with tempfile.TemporaryDirectory(dir=os.path.realpath(tempfile.gettempdir())) as root:
            explicit = Path(root, 'explicit-output')
            ambient = Path(root, 'ambient-output')
            ambient_files = {
                name: Path(root, name.lower())
                for name in ('GITHUB_ENV', 'GITHUB_PATH', 'GITHUB_STATE', 'GITHUB_STEP_SUMMARY')
            }
            environment = dict(data[0], RUNNER_TEMP=root, GITHUB_OUTPUT=str(explicit))
            ambient_environment = {
                'RUNNER_TEMP': root,
                'GITHUB_OUTPUT': str(ambient),
                **{name: str(path) for name, path in ambient_files.items()},
            }
            with patch.dict(os.environ, ambient_environment):
                decoded_source, arguments, prefix = TRANSPORT.decode_transport(
                    environment, *data[1:], ['GITHUB_OUTPUT', 'RUNNER_TEMP']
                )
                self.assertEqual(prefix[2:4], [
                    f'GITHUB_OUTPUT={explicit}', f'RUNNER_TEMP={root}',
                ])
                result = LAUNCHER.execute(
                    decoded_source, data[1], arguments, prefix
                )
            self.assertEqual(result, 0)
            self.assertEqual(explicit.read_text(), 'result=ok\n')
            self.assertFalse(ambient.exists())
            self.assertTrue(all(not path.exists() for path in ambient_files.values()))


if __name__ == '__main__':
    unittest.main()

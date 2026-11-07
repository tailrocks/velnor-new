# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import ast
import base64
import importlib.util
import json
import os
from pathlib import Path
import sys
import time
import unittest
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_publish_entry_test import context

ROOT = Path(__file__).resolve().parents[1] / 'src'
spec = importlib.util.spec_from_file_location('first_stage_contract', ROOT / 'release_publish_first_stage.py')
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)
BUNDLE = json.loads(Path(sys.argv[1]).read_bytes())
EXPORTED = Path(sys.argv[2]).read_bytes()
ENTRY = stage.bootstrap_entry(BUNDLE)
VERIFIER = ENTRY['module']('actual_export_contract', ENTRY['EXPORT_VERIFIER'])
CONTRACT = BUNDLE['export_contract']
VALUES = VERIFIER.constants(EXPORTED, {'SUPERVISOR', *[item['constant'] for item in CONTRACT['bootstrap']]})


def constant(source, name, value):
    node = next(item for item in ast.parse(source).body
                if isinstance(item, ast.Assign) and len(item.targets) == 1
                and isinstance(item.targets[0], ast.Name) and item.targets[0].id == name)
    lines = source.splitlines(keepends=True)
    start = sum(map(len, lines[:node.value.lineno - 1])) + node.value.col_offset
    end = sum(map(len, lines[:node.value.end_lineno - 1])) + node.value.end_col_offset
    return source[:start] + repr(value).encode() + source[end:]


def deadline():
    return time.clock_gettime_ns(time.CLOCK_BOOTTIME) + 30_000_000_000


class ExportContractTest(unittest.TestCase):
    def test_real_current_entry_matches_complete_closure(self):
        VERIFIER.verify(EXPORTED, ENTRY, 'publish', deadline())

    def test_rendered_and_bootstrap_entries_enforce_workflow_identity(self):
        rendered = {'__name__': 'rendered_entry'}
        exec(compile(EXPORTED, '<rendered-entry>', 'exec'), rendered)
        for entry in (rendered, ENTRY):
            for mode in ('trusted-publishing', 'bootstrap'):
                values = context(mode)
                with self.subTest(entry=entry['__name__'], mode=mode), patch.dict(os.environ, values, clear=True):
                    self.assertEqual(
                        entry['read_context'](mode)['GITHUB_WORKFLOW_REF'],
                        values['GITHUB_WORKFLOW_REF'],
                    )
                values['GITHUB_WORKFLOW_REF'] = 'example/project/.github/workflows/release.yml@refs/heads/main'
                for key in ('CARGO_REGISTRY_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN',
                            'ACTIONS_ID_TOKEN_REQUEST_URL'):
                    values.pop(key, None)
                with self.subTest(entry=entry['__name__'], mode=mode), patch.dict(os.environ, values, clear=True):
                    with self.assertRaises(ValueError):
                        entry['read_context'](mode)

    def test_each_bootstrap_module_is_bound(self):
        for item in CONTRACT['bootstrap']:
            with self.subTest(module=item['constant']):
                altered = base64.b64encode(base64.b64decode(VALUES[item['constant']]) + b'\n# changed\n').decode()
                with self.assertRaisesRegex(ValueError, '^exported module bytes mismatch$'):
                    VERIFIER.verify(constant(EXPORTED, item['constant'], altered), ENTRY, 'publish', deadline())

    def test_each_supervisor_module_is_bound(self):
        for name in CONTRACT['supervisor']:
            with self.subTest(module=name):
                payload = json.loads(base64.b64decode(VALUES['SUPERVISOR']))
                payload[name] = base64.b64encode(base64.b64decode(payload[name]) + b'\n# changed\n').decode()
                encoded = base64.b64encode(json.dumps(payload, sort_keys=True, separators=(',', ':')).encode()).decode()
                with self.assertRaisesRegex(ValueError, '^exported supervisor bytes mismatch$'):
                    VERIFIER.verify(constant(EXPORTED, 'SUPERVISOR', encoded), ENTRY, 'publish', deadline())

    def test_old_duplicate_unknown_and_outer_mutations_are_denied(self):
        node = next(item for item in ast.parse(EXPORTED).body
                    if isinstance(item, ast.Assign) and isinstance(item.targets[0], ast.Name)
                    and item.targets[0].id == 'EXPORT_CONTRACT')
        lines = EXPORTED.splitlines(keepends=True)
        old = b''.join(lines[:node.lineno - 1] + lines[node.end_lineno:])
        duplicate = EXPORTED + b"\nMANAGER = 'e30='\n"
        payload = json.loads(base64.b64decode(VALUES['SUPERVISOR']))
        payload['foreign'] = base64.b64encode(b'# foreign module').decode()
        encoded = base64.b64encode(json.dumps(payload, sort_keys=True, separators=(',', ':')).encode()).decode()
        for source, message in [(old, 'exported closure constants unavailable'),
                                (duplicate, 'exported closure constants unavailable'),
                                (constant(EXPORTED, 'SUPERVISOR', encoded), 'exported supervisor closure shape'),
                                (EXPORTED + b'\n# changed outer entry\n', 'exported outer entry mismatch')]:
            with self.subTest(denial=message):
                with self.assertRaisesRegex(ValueError, '^' + message + '$'):
                    VERIFIER.verify(source, ENTRY, 'publish', deadline())

    def test_expired_original_budget_is_rejected(self):
        with self.assertRaisesRegex(ValueError, '^exported closure deadline$'):
            VERIFIER.verify(EXPORTED, ENTRY, 'publish', time.clock_gettime_ns(time.CLOCK_BOOTTIME) - 1)


if __name__ == '__main__':
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(ExportContractTest)
    if suite.countTestCases() != 6:
        raise SystemExit('export contract cases missing')
    raise SystemExit(not unittest.TextTestRunner().run(suite).wasSuccessful())

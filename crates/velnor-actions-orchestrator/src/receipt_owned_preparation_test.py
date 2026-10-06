"""Phase boundaries: fixture witnesses grant no production warmth."""
import sys
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"))
import receipt_owned_preparation as preparation
from cache_receipt_common import ColdReceipt
from cache_receipt_materialize_transaction import MaterializationStop

_STAGES = tuple((name, (), ()) for name in ('clear', 'bootstrap', 'install', 'warm'))
_GH = SimpleNamespace(close=lambda: None)


class PreparationTests(unittest.TestCase):
    def test_stage_environment_drops_auth_startup_proxy_and_unknown_keys(self):
        hostile = {name: 'hostile' for name in ('GH_TOKEN', 'GITHUB_TOKEN', 'LD_PRELOAD',
                   'PYTHONPATH', 'BASH_ENV', 'SHELLOPTS', 'HTTPS_PROXY', 'FUTURE_SETTING')}
        fixed = {'RUNNER_TEMP': '/private/runner/temp', 'GITHUB_PATH': '/private/runner/path',
                 'GITHUB_OUTPUT': '/private/runner/output', 'OWNER_BINDING': 'qualified'}
        with patch.dict(os.environ, {**hostile, **fixed}, clear=True):
            with patch.object(preparation.subprocess, 'run') as run:
                preparation._run_source(('fixed source', (), (('MISE_DATA_DIR', 'OWNER_BINDING'),)))
        environment = run.call_args.kwargs['env']
        self.assertFalse(set(hostile) & set(environment))
        self.assertEqual(environment['MISE_DATA_DIR'], 'qualified')
        self.assertEqual(environment['PATH'], '/usr/bin:/bin:/usr/sbin:/sbin')

    def test_unqualified_source_ignores_writable_warm_observations(self):
        with patch.dict(os.environ, VELNOR_CACHE_VERIFIED='true',
                        VELNOR_RECEIPT_WARM='true', GH_TOKEN='fixture-token'):
            with patch.object(preparation, '_run_source') as run:
                preparation.prepare_owned(_STAGES)
        self.assertEqual([call.args[0][0] for call in run.call_args_list],
                         ['clear', 'bootstrap', 'install'])

    def test_cold_owner_failure_cannot_retry_or_enter_warm(self):
        for failing in ('clear', 'bootstrap', 'install'):
            with self.subTest(stage=failing):
                visited = []
                def run(stage):
                    visited.append(stage[0])
                    if stage[0] == failing:
                        raise OSError('fixture owner failure')
                with patch.object(preparation, '_run_source', side_effect=run):
                    with self.assertRaises(OSError):
                        preparation.prepare_owned(_STAGES)
                self.assertEqual(visited[-1], failing)
                self.assertEqual(len(visited), len(set(visited)))
                self.assertNotIn('warm', visited)

    def test_only_receipt_rejection_chooses_cold_after_clear(self):
        namespace = SimpleNamespace(close=lambda: None)
        context = SimpleNamespace(quarantine='fixture', gh=_GH, policy=None)
        with patch.object(preparation, '_qualified_runtime', return_value=context):
            with patch.object(preparation, '_capture_source_namespace', return_value=namespace):
                with patch.object(preparation, 'verify_payload',
                                  side_effect=ColdReceipt('receipt_rejected')):
                    with patch.object(preparation, '_run_source') as run:
                        preparation.prepare_owned(_STAGES)
        self.assertEqual([call.args[0][0] for call in run.call_args_list],
                         ['clear', 'bootstrap', 'install'])

    def test_nonreceipt_pregrant_failure_is_terminal(self):
        closed = []
        namespace = SimpleNamespace(close=lambda: closed.append(True))
        context = SimpleNamespace(quarantine='fixture', gh=_GH, policy=None)
        with patch.object(preparation, '_qualified_runtime', return_value=context):
            with patch.object(preparation, '_capture_source_namespace', return_value=namespace):
                with patch.object(preparation, 'verify_payload', side_effect=RuntimeError('fault')):
                    with patch.object(preparation, '_run_source') as run:
                        with self.assertRaises(RuntimeError):
                            preparation.prepare_owned(_STAGES)
        self.assertEqual([call.args[0][0] for call in run.call_args_list], ['clear'])
        self.assertEqual(closed, [True])

    def test_actual_fixture_materialization_then_terminal_warm_failure(self):
        from cache_receipt_materialize_test import MaterializeTests
        fixture = MaterializeTests()
        with tempfile.TemporaryDirectory() as temporary:
            with patch('source_archive_inventory_leaf.metadata_records', return_value=[]):
                runner, _, grant, policy = fixture.prepare(Path(temporary).resolve())
            binding, environment = fixture.capture(runner, policy)
            context = SimpleNamespace(quarantine=grant.quarantine, gh=_GH, policy=policy)
            visited = []
            def run(stage):
                visited.append(stage[0])
                if stage[0] == 'warm':
                    self.assertTrue((runner / 'velnor/cargo/bin/tool').exists())
                    raise OSError('terminal health failure')
            with binding, environment:
                with patch('source_archive_inventory_leaf.metadata_records', return_value=[]):
                    with patch('cache_receipt_materialize.metadata_records', return_value=[]):
                        with patch.object(preparation, '_qualified_runtime', return_value=context):
                            with patch.object(preparation, 'verify_payload', return_value=grant):
                                with patch.object(preparation, '_run_source', side_effect=run):
                                    with self.assertRaisesRegex(OSError, 'terminal health failure'):
                                        preparation.prepare_owned(_STAGES)
            self.assertEqual(visited, ['clear', 'warm'])

    def test_postgrant_materialization_failure_cannot_select_cold(self):
        from cache_receipt_materialize_test import MaterializeTests
        fixture = MaterializeTests()
        with tempfile.TemporaryDirectory() as temporary:
            with patch('source_archive_inventory_leaf.metadata_records', return_value=[]):
                runner, _, grant, policy = fixture.prepare(Path(temporary).resolve())
            binding, environment = fixture.capture(runner, policy)
            context = SimpleNamespace(quarantine=grant.quarantine, gh=_GH, policy=policy)
            with binding, environment, patch('cache_receipt_materialize.metadata_records',
                                             return_value=[]):
                with patch.object(preparation, '_qualified_runtime', return_value=context):
                    with patch.object(preparation, 'verify_payload', return_value=grant):
                        with patch.object(preparation, '_materialize_verified',
                                          side_effect=ColdReceipt('changed_after_grant')):
                            with patch.object(preparation, '_run_source') as run:
                                with self.assertRaisesRegex(ColdReceipt, 'changed_after_grant'):
                                    preparation.prepare_owned(_STAGES)
            self.assertEqual([call.args[0][0] for call in run.call_args_list], ['clear'])

    def test_forged_grant_never_launches_warm(self):
        with patch.object(preparation, '_run_source') as run:
            with self.assertRaises(MaterializationStop):
                preparation._warm(object(), object(), _STAGES[3])
        run.assert_not_called()


if __name__ == '__main__':
    unittest.main()

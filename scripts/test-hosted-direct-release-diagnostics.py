# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import json
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    'hosted_direct_release', pathlib.Path(__file__).with_name('check-hosted-direct-release.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def parse(lines):
    try:
        module.validate_results(b'\n'.join(lines))
    except SystemExit as error:
        return str(error)
    return 'accepted'


CAPTURED_EVENTS = [{'type': 'suite', 'event': 'started', 'test_count': 9},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$image_maintenance::publication_boundary_worker::promotion'},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$image_maintenance::publication_boundary_worker::publication'},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_semver::lockless_qualified_tests::qualified_checker_accepts_lockless_baseline_and_rejects_breakage'},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_semver::lockless_qualified_tests::qualified_checker_checks_shared_features_and_allows_new_features'},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::publish::publisher_runs_guarded_durable_coordinator_in_order'},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::publish::direct_publisher_rejects_pin_environment_and_order_tampering'},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport'},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::render::early::rendered_workflow_has_one_hosted_environment_publisher'},
 {'type': 'test',
  'event': 'started',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_runtime::rehearsal::generated_source_require_rehearsal'},
 {'type': 'test',
  'event': 'ignored',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$image_maintenance::publication_boundary_worker::promotion'},
 {'type': 'test',
  'event': 'ignored',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$image_maintenance::publication_boundary_worker::publication'},
 {'type': 'test',
  'event': 'ignored',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_semver::lockless_qualified_tests::qualified_checker_accepts_lockless_baseline_and_rejects_breakage'},
 {'type': 'test',
  'event': 'ignored',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_semver::lockless_qualified_tests::qualified_checker_checks_shared_features_and_allows_new_features'},
 {'type': 'test',
  'event': 'ok',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::render::early::rendered_workflow_has_one_hosted_environment_publisher'},
 {'type': 'suite', 'event': 'ok', 'passed': 1, 'failed': 0, 'ignored': 4},
 {'type': 'suite', 'event': 'started', 'test_count': 9},
 {'type': 'test',
  'event': 'ok',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::publish::publisher_runs_guarded_durable_coordinator_in_order'},
 {'type': 'test',
  'event': 'ok',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::publish::direct_publisher_rejects_pin_environment_and_order_tampering'},
 {'type': 'test',
  'event': 'ok',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_runtime::rehearsal::generated_source_require_rehearsal'},
 {'type': 'test',
  'event': 'ok',
  'name': 'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport'},
 {'type': 'suite', 'event': 'ok', 'passed': 4, 'failed': 0, 'ignored': 4}]

def parse_events(events):
    return parse([json.dumps(item).encode() for item in events])


class DiagnosticsTest(unittest.TestCase):
    def test_complete_results_are_accepted(self):
        self.assertEqual('accepted', parse_events(CAPTURED_EVENTS))

    def test_mutated_results_are_rejected(self):
        import copy
        cases = []
        for index, event in enumerate(CAPTURED_EVENTS):
            if event['type'] == 'test' and event['event'] in ('ok', 'ignored'):
                missing_event = copy.deepcopy(CAPTURED_EVENTS)
                del missing_event[index]
                cases.append(missing_event)
        unknown = copy.deepcopy(CAPTURED_EVENTS)
        unknown[1]['name'] = 'unexpected'
        cases.append(unknown)
        missing = copy.deepcopy(CAPTURED_EVENTS)
        del missing[5]
        cases.append(missing)
        duplicate = copy.deepcopy(CAPTURED_EVENTS)
        duplicate.insert(2, copy.deepcopy(duplicate[1]))
        cases.append(duplicate)
        ignored_pass = copy.deepcopy(CAPTURED_EVENTS)
        ignored_pass[10]['event'] = 'ok'
        cases.append(ignored_pass)
        wrong_summary = copy.deepcopy(CAPTURED_EVENTS)
        wrong_summary[15]['passed'] = 2
        cases.append(wrong_summary)
        for events in cases:
            with self.subTest(events=events):
                self.assertNotEqual('accepted', parse_events(events))

    def test_bad_json_reports_index_without_content(self):
        result = parse([b'{"type":"suite","event":"started","test_count":9}', b'secret-token'])
        self.assertIn('category=invalid_json record_index=2', result)
        self.assertNotIn('secret-token', result)

    def test_suite_size_reports_index_without_content(self):
        result = parse([b'{"type":"suite","event":"started","test_count":99,"secret":"sensitive"}'])
        self.assertIn('category=suite_size record_index=1', result)
        self.assertNotIn('sensitive', result)

    def test_missing_events_fail_closed(self):
        self.assertIn('category=test_results record_index=0', parse([]))

    def test_unexpected_test_fails_closed_without_name(self):
        result = parse([b'{"type":"suite","event":"started","test_count":9}',
                        b'{"type":"test","event":"started","name":"secret-test-name"}'])
        self.assertIn('category=test_event record_index=2', result)
        self.assertNotIn('secret-test-name', result)


if __name__ == '__main__':
    unittest.main()

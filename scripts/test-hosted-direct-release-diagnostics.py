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

HISTORICAL_WORKER = ('velnor-actions-orchestrator::velnor_actions_orchestrator$crate_release_controller::'
                     'tests::signed_history::workspace::exercise')
CAPTURED_EVENTS.insert(1, {'type': 'test', 'event': 'started', 'name': HISTORICAL_WORKER})
for event in CAPTURED_EVENTS:
    if event['type'] == 'suite' and event['event'] == 'started':
        event['test_count'] = 10
    if event['type'] == 'suite' and event['event'] == 'ok':
        event['ignored'] = 5

def parse_events(events):
    return parse([json.dumps(item).encode() for item in events])


class DiagnosticsTest(unittest.TestCase):
    def test_captured_linux_failure_rejected_and_synthetic_success_accepted(self):
        fixture = pathlib.Path(__file__).parent / 'tests/fixtures/hosted-direct-release-linux-failed.jsonl'
        events = [json.loads(line) for line in fixture.read_text().splitlines()]
        self.assertNotEqual('accepted', parse_events(events))
        for event in events:
            if event.get('event') == 'failed':
                event['event'] = 'ok'
                if event['type'] == 'suite':
                    event['passed'] = 5
                    event['failed'] = 0
        self.assertEqual('accepted', parse_events(events))
        missing = [item for item in events if item.get('name') != HISTORICAL_WORKER]
        self.assertNotEqual('accepted', parse_events(missing))
        import copy
        for mutation in ('duplicate', 'unknown', 'ignored_success', 'count', 'summary', 'missing_required'):
            changed = copy.deepcopy(events)
            if mutation == 'duplicate':
                changed.insert(2, copy.deepcopy(changed[1]))
            elif mutation == 'unknown':
                changed[1]['name'] = 'unexpected'
            elif mutation == 'ignored_success':
                changed.insert(2, {'type': 'test', 'event': 'ok', 'name': HISTORICAL_WORKER})
            elif mutation == 'count':
                changed[0]['test_count'] = 9
            elif mutation == 'summary':
                next(item for item in changed if item['type'] == 'suite' and item['event'] == 'ok')['ignored'] = 4
            else:
                changed = [item for item in changed if not item.get('name', '').endswith('test_only_dispatch_and_loopback_transport')]
            with self.subTest(mutation=mutation):
                self.assertNotEqual('accepted', parse_events(changed))

    def test_complete_results_are_accepted(self):
        self.assertEqual('accepted', parse_events(CAPTURED_EVENTS))

    def test_mutated_results_are_rejected(self):
        import copy
        cases = []
        for index, event in enumerate(CAPTURED_EVENTS):
            if event['type'] == 'test' and event['event'] == 'ok':
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
        next(event for event in ignored_pass if event.get('event') == 'ignored')['event'] = 'ok'
        cases.append(ignored_pass)
        wrong_summary = copy.deepcopy(CAPTURED_EVENTS)
        next(event for event in wrong_summary if event['type'] == 'suite' and event['event'] == 'ok')['passed'] = 2
        cases.append(wrong_summary)
        for events in cases:
            with self.subTest(events=events):
                self.assertNotEqual('accepted', parse_events(events))

    def test_bad_json_reports_index_without_content(self):
        result = parse([b'{"type":"suite","event":"started","test_count":10}', b'secret-token'])
        self.assertIn('category=invalid_json record_index=2', result)
        self.assertNotIn('secret-token', result)

    def test_suite_size_reports_index_without_content(self):
        result = parse([b'{"type":"suite","event":"started","test_count":99,"secret":"sensitive"}'])
        self.assertIn('category=suite_size record_index=1', result)
        self.assertNotIn('sensitive', result)

    def test_missing_events_fail_closed(self):
        self.assertIn('category=test_results record_index=0', parse([]))

    def test_unexpected_test_fails_closed_without_name(self):
        result = parse([b'{"type":"suite","event":"started","test_count":10}',
                        b'{"type":"test","event":"started","name":"secret-test-name"}'])
        self.assertIn('category=test_event record_index=2', result)
        self.assertNotIn('secret-test-name', result)


if __name__ == '__main__':
    unittest.main()

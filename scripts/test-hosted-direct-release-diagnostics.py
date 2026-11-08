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


class DiagnosticsTest(unittest.TestCase):
    def test_complete_results_are_accepted(self):
        prefix = 'velnor-actions-orchestrator::velnor_actions_orchestrator$'
        names = [
            prefix + 'release::release_steps::tests::publish::publisher_runs_guarded_durable_coordinator_in_order',
            prefix + 'release::release_steps::tests::publish::direct_publisher_rejects_pin_environment_and_order_tampering',
            prefix + 'release::release_steps::tests::render::early::rendered_workflow_has_one_hosted_environment_publisher',
            prefix + 'release::crate_release_runtime::rehearsal::generated_source_require_rehearsal',
            prefix + 'release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport',
        ]
        ignored = [prefix + 'image_maintenance::publication_boundary_worker::' + name
                   for name in ('promotion', 'publication')]
        events = [{'type': 'suite', 'event': 'started', 'test_count': 7}]
        for name in names + ignored:
            events.append({'type': 'test', 'event': 'started', 'name': name})
        for name in names:
            events.append({'type': 'test', 'event': 'ok', 'name': name})
        for name in ignored:
            events.append({'type': 'test', 'event': 'ignored', 'name': name})
        events.append({'type': 'suite', 'event': 'ok', 'passed': 5, 'failed': 0, 'ignored': 2})
        self.assertEqual('accepted', parse([json.dumps(item).encode() for item in events]))

    def test_bad_json_reports_index_without_content(self):
        result = parse([b'{"type":"suite","event":"started","test_count":7}', b'secret-token'])
        self.assertIn('category=invalid_json record_index=2', result)
        self.assertNotIn('secret-token', result)

    def test_suite_size_reports_index_without_content(self):
        result = parse([b'{"type":"suite","event":"started","test_count":99,"secret":"sensitive"}'])
        self.assertIn('category=suite_size record_index=1', result)
        self.assertNotIn('sensitive', result)

    def test_missing_events_fail_closed(self):
        self.assertIn('category=test_results record_index=0', parse([]))

    def test_unexpected_test_fails_closed_without_name(self):
        result = parse([b'{"type":"suite","event":"started","test_count":7}',
                        b'{"type":"test","event":"started","name":"secret-test-name"}'])
        self.assertIn('category=test_event record_index=2', result)
        self.assertNotIn('secret-test-name', result)


if __name__ == '__main__':
    unittest.main()

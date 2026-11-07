# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Run the direct crate-release regressions on the GitHub-hosted runner."""

import json
import os
import platform
import subprocess

with open('/etc/os-release', encoding='utf-8') as release_file:
    release = release_file.read().splitlines()
if ('ID=ubuntu' not in release or 'VERSION_ID="26.04"' not in release
        or platform.machine() != 'x86_64'
        or os.environ.get('VELNOR_CHECK_PLATFORM') != 'linux_x64'):
    raise SystemExit('direct-release qualification requires GitHub-hosted Ubuntu 26.04 x86_64')

tests = {
    'publisher_runs_guarded_durable_coordinator_in_order',
    'direct_publisher_rejects_pin_environment_and_order_tampering',
    'rendered_workflow_has_one_hosted_environment_publisher',
    'generated_source_require_rehearsal',
    'test_only_dispatch_and_loopback_transport',
}
expression = ' | '.join('test(' + name + ')' for name in sorted(tests))
env = dict(os.environ, NEXTEST_EXPERIMENTAL_LIBTEST_JSON='1')
for key in list(env):
    if key.startswith('VELNOR_TEST_'):
        env.pop(key)
for key in ('VELNOR_TEST_PINNED_GIT', 'VELNOR_TEST_PINNED_GIT_HTTP', 'GITHUB_TOKEN',
            'GH_TOKEN', 'CARGO_REGISTRY_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN',
            'ACTIONS_ID_TOKEN_REQUEST_URL', 'ACTIONS_RUNTIME_TOKEN'):
    env.pop(key, None)
result = subprocess.run(
    ['cargo', 'nextest', 'run', '--locked', '-p', 'velnor-actions-orchestrator', '--lib',
     '-E', expression, '--message-format', 'libtest-json'],
    env=env, capture_output=True, timeout=780, check=False,
)
if result.returncode != 0 or len(result.stdout) > 65536:
    failed = set()
    if len(result.stdout) <= 65536:
        for line in result.stdout.splitlines():
            try:
                event = json.loads(line)
                if event.get('type') == 'test' and event.get('event') == 'failed':
                    name = event.get('name', '').rsplit('::', 1)[-1]
                    if name in tests:
                        failed.add(name)
            except (ValueError, TypeError, AttributeError):
                pass
    details = ','.join(sorted(failed)) if failed else 'unavailable'
    raise SystemExit(f'direct-release tests failed: status={result.returncode} '
                     f'stdout_bytes={len(result.stdout)} failed_tests={details}')

def unique(pairs):
    data = {}
    for key, value in pairs:
        if key in data:
            raise ValueError('duplicate JSON key')
        data[key] = value
    return data

try:
    records = [json.loads(line, object_pairs_hook=unique) for line in result.stdout.splitlines()]
    suites = [r for r in records if r.get('type') == 'suite']
    events = [r for r in records if r.get('type') == 'test']
    expected = {'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::publish::'
                'publisher_runs_guarded_durable_coordinator_in_order',
                'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::publish::'
                'direct_publisher_rejects_pin_environment_and_order_tampering',
                'velnor-actions-orchestrator::velnor_actions_orchestrator$release::release_steps::tests::render::early::'
                'rendered_workflow_has_one_hosted_environment_publisher',
                'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_runtime::rehearsal::'
                'generated_source_require_rehearsal',
                'velnor-actions-orchestrator::velnor_actions_orchestrator$release::crate_release_runtime::tests::'
                'test_only_dispatch_and_loopback_transport'}
    started = [r.get('name') for r in events if r.get('event') == 'started']
    passed = [r.get('name') for r in events if r.get('event') == 'ok']
    if (len(records) != 12 or len(suites) != 2
            or suites[0].get('event') != 'started' or suites[0].get('test_count') != len(tests)
            or suites[1].get('event') != 'ok' or suites[1].get('passed') != len(tests)
            or suites[1].get('failed') != 0 or len(events) != 10
            or len(started) != len(tests) or set(started) != expected
            or len(passed) != len(tests) or set(passed) != expected):
        raise ValueError('unexpected test results')
except (ValueError, TypeError, AttributeError):
    raise SystemExit('direct-release qualification returned incomplete results') from None

raise SystemExit('direct-release qualification is incomplete: live protected OIDC and '
                 'interrupted-publication recovery have not been exercised')

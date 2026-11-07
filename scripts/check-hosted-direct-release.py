# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Run the direct crate-release regressions on the GitHub-hosted runner."""

import json
import os
import platform
import subprocess
import tempfile
from collections import Counter

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
            'ACTIONS_ID_TOKEN_REQUEST_URL', 'ACTIONS_RUNTIME_TOKEN', 'VELNOR_GH_FIXTURE_TOOLS'):
    env.pop(key, None)
if not os.path.isabs(env.get('HOME', '')):
    raise SystemExit('direct-release qualification requires an absolute private HOME')
with tempfile.TemporaryDirectory(prefix='hosted-direct-release-', dir=env['HOME']) as runner_temp:
    env['RUNNER_TEMP'] = runner_temp
    result = subprocess.run(
        ['cargo', 'nextest', 'run', '--locked', '-p', 'velnor-actions-orchestrator', '--lib',
         '-E', expression, '--message-format', 'libtest-json'],
        env=env, capture_output=True, timeout=780, check=False,
    )
if result.returncode != 0 or len(result.stdout) > 65536:
    failed = set()
    failure_kinds = set()
    if len(result.stdout) <= 65536:
        markers = {
            b'set VELNOR_GH_FIXTURE_TOOLS to an absolute directory': 'fixture_tools_missing',
            b'fixture tool directory is incomplete': 'fixture_tools_incomplete',
            b'fixture tool staging failed:': 'fixture_tools_staging',
            b'trusted local Docker client is unavailable': 'docker_client_unavailable',
            b'fixture Docker daemon must use a local Unix socket': 'docker_socket_invalid',
            b'fixture Docker image digest mismatch': 'docker_image_mismatch',
            b'No such image:': 'docker_image_missing',
            b'fixture source capture failed': 'fixture_capture_failed',
            b'fixture manifest read failed:': 'fixture_manifest_read',
            b'fixture capture verification failed:': 'fixture_capture_verify',
            b'fixture bundled inputs failed:': 'fixture_bundle_verify',
            b'fixture reviewed exclusions failed:': 'fixture_exclusion_verify',
            b'isolated image attestation fixture failed:': 'fixture_worker_exit',
            b'panicked at': 'fixture_panic',
            b'isolated GitHub fixture:': 'fixture_driver_error',
            b'invalid captured fixture source': 'fixture_capture_verification',
            b'captured fixture code differs from compiled harness': 'fixture_code_mismatch',
            b'captured fixture staging differs from compiled harness': 'fixture_staging_mismatch',
            b'fixture capture manifest changed': 'fixture_manifest_changed',
            b'fixture dependency source is unavailable': 'fixture_dependency_unavailable',
            b'fixture dependency sources exceed fingerprint bounds': 'fixture_dependency_bounds',
            b'offline Cargo registry is unavailable': 'fixture_registry_unavailable',
            b'fixture tool does not match its qualified digest': 'fixture_tool_digest',
            b'fixture tool cache is corrupt': 'fixture_tool_cache',
            b'fixture cache must be a private owned directory': 'fixture_cache_permissions',
            b'fixture sources changed during isolated build': 'fixture_build_source_changed',
            b'fixture sources changed during isolated execution': 'fixture_execution_source_changed',
            b'fixture cache identity mismatch': 'fixture_cache_identity',
            b'fixture worker success receipt does not match scenario': 'fixture_receipt_mismatch',
            b'isolated image attestation fixture failed': 'fixture_child_failed',
            b'error: could not compile': 'fixture_compile_failed',
            b'fixture server panicked': 'fixture_server_panicked',
            b'fixture child failed': 'fixture_transport_child_failed',
            b'fixture server:': 'fixture_server_failed',
            b'No such file or directory': 'missing_file',
            b'Permission denied': 'permission_denied',
            b'Connection refused': 'connection_refused',
            b'timed out': 'timeout',
            b'fixture tracked sources changed': 'fixture_source_changed',
            b'release Git package extraction failed': 'fixture_git_extraction',
            b'crate_release_history_custody_required': 'history_custody_required',
            b'crate_release_missing_GITHUB_EVENT_PATH': 'dispatch_event_missing',
        }
        for line in result.stdout.splitlines():
            try:
                event = json.loads(line)
                if event.get('type') == 'test' and event.get('event') == 'failed':
                    name = event.get('name', '').rsplit('::', 1)[-1]
                    if name in tests:
                        failed.add(name)
                        failure_kinds.update(label for marker, label in markers.items() if marker in line)
            except (ValueError, TypeError, AttributeError):
                pass
    details = ','.join(sorted(failed)) if failed else 'unavailable'
    kinds = ','.join(sorted(failure_kinds)) if failure_kinds else 'unclassified'
    raise SystemExit(f'direct-release tests failed: status={result.returncode} '
                     f'stdout_bytes={len(result.stdout)} failed_tests={details} failure_hints={kinds}')

def unique(pairs):
    data = {}
    for key, value in pairs:
        if key in data:
            raise ValueError('duplicate JSON key')
        data[key] = value
    return data

try:
    records = [json.loads(line, object_pairs_hook=unique) for line in result.stdout.splitlines()]
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
    ignored = {
        'velnor-actions-orchestrator::velnor_actions_orchestrator$image_maintenance::publication_boundary_worker::promotion',
        'velnor-actions-orchestrator::velnor_actions_orchestrator$image_maintenance::publication_boundary_worker::publication',
    }
    starts, passes, skips = Counter(), Counter(), Counter()
    suite_open = False
    suite_count = 0
    suite_passed = 0
    suite_events = 0
    suite_actual_passed = 0
    for record in records:
        kind, event = record.get('type'), record.get('event')
        if kind == 'suite' and event == 'started' and not suite_open:
            if record.get('test_count') != len(expected) + len(ignored):
                raise ValueError('unexpected suite size')
            suite_open = True
            suite_count += 1
            suite_events = 0
            suite_actual_passed = 0
        elif kind == 'suite' and event == 'ok' and suite_open:
            passed = record.get('passed')
            if (type(passed) is not int or passed < 0 or type(record.get('failed')) is not int
                    or record.get('failed') != 0 or suite_events == 0
                    or passed != suite_actual_passed
                    or record.get('ignored') != len(ignored)):
                raise ValueError('unexpected suite result')
            suite_passed += passed
            suite_open = False
        elif kind == 'test' and suite_open:
            suite_events += 1
            name = record.get('name')
            if event == 'started' and name in expected | ignored:
                starts[name] += 1
            elif event == 'ok' and name in expected and starts[name] == 1:
                passes[name] += 1
                suite_actual_passed += 1
            elif event == 'ignored' and name in ignored and starts[name] == 1:
                skips[name] += 1
            else:
                raise ValueError('unexpected test event')
        else:
            raise ValueError('unexpected record')
    if (suite_open or not 1 <= suite_count <= len(expected) + len(ignored)
            or suite_passed != len(expected)
            or starts != Counter({name: 1 for name in expected | ignored})
            or passes != Counter({name: 1 for name in expected})
            or skips != Counter({name: 1 for name in ignored})):
        raise ValueError('unexpected test results')
except (ValueError, TypeError, AttributeError):
    raise SystemExit('direct-release qualification returned incomplete results') from None

raise SystemExit('direct-release qualification is incomplete: live protected OIDC and '
                 'interrupted-publication recovery have not been exercised')

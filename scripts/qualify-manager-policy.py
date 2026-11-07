# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
RENDERER = ROOT / 'crates/velnor-actions-workflow-renderer'
FIXTURE = RENDERER / 'tests/manager_policy_fixture'


def command(arguments, timeout=300):
    result = subprocess.run(arguments, capture_output=True, timeout=timeout)
    if len(result.stdout) + len(result.stderr) > 2 * 1024 * 1024:
        raise ValueError('qualification command output bound')
    if result.returncode:
        raise ValueError('qualification command failed: ' + str(arguments[0]) + '\n'
                         + result.stderr.decode('utf-8', errors='replace'))
    return result


def assert_tests(result, count):
    output = result.stderr.decode('utf-8', errors='strict')
    if (not re.search(r'\nRan ' + str(count) + r' tests? in [0-9.]+s\n\nOK\n\Z', output)
            or 'skipped' in output):
        raise ValueError('qualification did not execute the exact case inventory')


def require_host():
    release = Path('/etc/os-release').read_text().splitlines()
    if (os.uname().machine != 'x86_64' or 'ID=ubuntu' not in release
            or 'VERSION_ID="26.04"' not in release):
        raise ValueError('native Ubuntu26 x86_64 qualification required')


def clean_host():
    result = command(['/usr/bin/sudo', '-n', '/usr/bin/env', '-i', '/usr/bin/python3',
                      '-I', '-S', '-B', str(RENDERER / 'tests/release_manager_policy_systemd_test.py')], 45)
    assert_tests(result, 1)


def wait_bus(container):
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        ready = True
        for unit in ('dbus.service', 'fixture-manager-environment.service'):
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('qualification readiness deadline')
            result = subprocess.run(['docker', 'exec', container, '/usr/bin/systemctl',
                                    'is-active', unit], capture_output=True,
                                    timeout=min(5, remaining))
            if result.stdout == b'failed\n':
                raise ValueError('qualification fixture service failed: ' + unit)
            ready = ready and result.returncode == 0 and result.stdout == b'active\n'
        if ready and time.monotonic() < deadline:
            return
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError('qualification readiness deadline')
        state = command(['docker', 'inspect', '--format', '{{.State.Running}}', container], min(5, remaining))
        if state.stdout != b'true\n':
            raise ValueError('qualification PID1 exited before DBus readiness')
        time.sleep(min(0.2, max(0, deadline - time.monotonic())))
    raise TimeoutError('qualification DBus readiness deadline')


def container_cases(context):
    tag = 'velnor-manager-policy-check:' + secrets.token_hex(16)
    command(['docker', 'build', '--tag', tag, str(context)], 600)
    container = None
    try:
        started = command(['docker', 'run', '--detach', '--privileged', '--cgroupns', 'private',
                           '--network', 'none', '--tmpfs', '/run', '--tmpfs', '/run/lock',
                           '--tmpfs', '/tmp', tag], 30)
        identity = started.stdout.decode('ascii').strip()
        if not re.fullmatch('[0-9a-f]{64}', identity):
            raise ValueError('qualification container identity')
        container = identity
        wait_bus(container)
        renderer = '/opt/velnor-policy/crates/velnor-actions-workflow-renderer'
        command(['docker', 'exec', container, '/usr/bin/mkdir', '-p', renderer + '/src',
                 renderer + '/tests', '/opt/velnor-policy/scripts'], 10)
        for directory, names in {
                'src': ('release_manager_bus.py', 'release_manager_lifecycle.py',
                        'release_manager_policy_host.py', 'release_manager_policy_files.py',
                        'release_manager_policy.py', 'release_watch_admission.py',
                        'release_bootstrap_watch.py', 'release_launcher_watch.py',
                        'release_supervisor_claim.py', 'release_supervisor_exec.py'),
                'tests': ('release_manager_policy_systemd_test.py', 'release_watch_fixture.py')}.items():
            for name in names:
                command(['docker', 'cp', str(RENDERER / directory / name),
                         container + ':' + renderer + '/' + directory + '/' + name], 10)
        command(['docker', 'cp', str(ROOT / 'scripts/observe-hosted-systemd-policy.py'),
                 container + ':/opt/velnor-policy/scripts/observe-hosted-systemd-policy.py'], 10)
        observation = command(['docker', 'exec', container, '/usr/bin/python3', '-I', '-S',
                               '-B', '/opt/velnor-policy/scripts/observe-hosted-systemd-policy.py', '--capture'], 20)
        print(observation.stdout.decode('utf-8', errors='strict'), file=sys.stderr, end='')
        result = command(['docker', 'exec', container, '/usr/bin/python3', '-I', '-S', '-B',
                          renderer + '/tests/release_manager_policy_systemd_test.py',
                          '--container'], 240)
        assert_tests(result, 5)
    finally:
        try:
            if container is not None:
                command(['docker', 'stop', '--time', '30', container], 40)
                state = command(['docker', 'inspect', '--format', '{{.State.Running}}', container], 10)
                if state.stdout != b'false\n':
                    raise ValueError('qualification container cleanup uncertain')
        finally:
            if container is not None:
                command(['docker', 'rm', '--force', container], 20)
            command(['docker', 'image', 'rm', tag], 20)


def main():
    require_host()
    clean_host()
    cache = Path(os.environ.get('RUNNER_TEMP', tempfile.gettempdir())) / 'velnor-manager-package-cache'
    with tempfile.TemporaryDirectory(prefix='velnor-manager-policy-check-') as temporary:
        output = Path(temporary)
        result = command(['/usr/bin/python3', '-I', '-S', '-B', str(FIXTURE / 'prepare.py'),
                          '--cache', str(cache), '--output', str(output)], 310)
        context = Path(result.stdout.decode('utf-8').strip())
        if context.parent != output or not context.is_dir():
            raise ValueError('qualification context identity')
        container_cases(context)
    evidence = {'schema': 1, 'source': 'mise-task-v1', 'head': os.environ['VELNOR_CHECK_HEAD'],
                'check_id': os.environ['VELNOR_CHECK_ID'], 'platform': os.environ['VELNOR_CHECK_PLATFORM'],
                'scenarios': [{'id': name, 'executed': True, 'status': 'passed'} for name in
                              ('isolated-manager-policy-negatives', 'native-manager-profile')]}
    (ROOT / 'hosted-manager-policy-evidence.json').write_text(json.dumps(evidence, sort_keys=True) + '\n')


if __name__ == '__main__':
    main()

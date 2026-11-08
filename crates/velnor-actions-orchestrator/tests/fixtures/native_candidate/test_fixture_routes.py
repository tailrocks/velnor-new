# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Verify the real loopback fixture distinguishes workflow and required-job routes."""
import json
import hashlib
import os
import pathlib
import struct
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request


def main():
    fixture = pathlib.Path(__file__).with_name('fixture.py')
    cargo = pathlib.Path(os.environ['VELNOR_FIXTURE_CARGO'])
    with tempfile.TemporaryDirectory(prefix='velnor-fixture-routes-') as temp:
        env = {'PATH': str(cargo.parent) + os.pathsep + '/usr/bin:/bin', 'HOME': temp,
               'RUNNER_TEMP': temp, 'CARGO_NET_OFFLINE': 'true', 'VELNOR_FIXTURE_REAL_CARGO': '1'}
        server = subprocess.Popen([sys.executable, str(fixture), temp], env=env,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            line = server.stdout.readline()
            if not line:
                raise RuntimeError('fixture did not start: ' + server.stderr.read())
            info = json.loads(line)
            prefix = f"{info['origin']}/repos/{info['repository']}"
            with urllib.request.urlopen(prefix + '/actions/runs/12', timeout=5) as response:
                run = json.load(response)
            with urllib.request.urlopen(prefix + '/actions/runs/123/attempts/1/jobs?per_page=100&page=1', timeout=5) as response:
                jobs = json.load(response)
            job = jobs['jobs'][0]
            assert run['id'] == 12 and jobs['total_count'] == 1
            assert (job['id'], job['run_id'], job['run_attempt'], job['name']) == (1, 123, 1, 'Required')
            assert job['check_run_url'] == f"https://api.github.com/repos/{info['repository']}/check-runs/1"
            source = pathlib.Path(temp) / 'velnor-crate-release-source'
            subprocess.run([str(cargo), 'package', '--offline', '--no-verify', '--allow-dirty'],
                           cwd=source, env=env, capture_output=True, check=True, timeout=60)
            archive = (source / 'target/package/example-0.1.1.crate').read_bytes()
            metadata = json.dumps({'name': 'example', 'vers': '0.1.1'}).encode()
            framed = struct.pack('<I', len(metadata)) + metadata + struct.pack('<I', len(archive)) + archive
            upload = info['origin'] + '/api/v1/crates/new'
            for body, token, status in [(framed, 'wrong-fixture-token', 403),
                                        (b'badframe', 'fixture-scoped', 400)]:
                request = urllib.request.Request(upload, data=body, method='PUT', headers={'Authorization': token})
                try:
                    urllib.request.urlopen(request, timeout=5)
                    raise AssertionError('invalid upload accepted')
                except urllib.error.HTTPError as error:
                    assert error.code == status
            request = urllib.request.Request(upload, data=framed, method='PUT', headers={'Authorization': 'fixture-scoped'})
            with urllib.request.urlopen(request, timeout=5) as response:
                assert response.status == 200
            with urllib.request.urlopen(info['origin'] + '/__fixture/state', timeout=5) as response:
                state = json.load(response)
            assert state['upload_attempts'] == 3 and state['uploads'] == 1
            assert state['published_sha256'] == hashlib.sha256(archive).hexdigest()
            assert (pathlib.Path(info['workspace']) / 'fixture-upload.crate').read_bytes() == archive
        finally:
            server.terminate()
            server.wait(timeout=10)


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import pathlib
import subprocess
import tempfile

IMAGE = "rust@sha256:4c4c8ec55be09785e6d6ac45ea9853525a73c15db74009100ac08952e773dfb8"
ROOT = pathlib.Path(__file__).resolve().parents[2]

FAKE_DOCKER = r'''#!/usr/bin/python3
import json, os, pathlib, socket, sys, time
args = sys.argv[1:]
case = pathlib.Path('/tmp/probe-case').read_text()
with open('/tmp/probe-calls', 'a') as stream:
    stream.write(json.dumps({'args': args, 'env': dict(os.environ)}) + '\n')
if args == ['--version']:
    if case == 'timeout':
        child = os.fork()
        if child == 0:
            time.sleep(30)
            pathlib.Path('/tmp/probe-child-survived').write_text('unexpected')
            os._exit(0)
        pathlib.Path('/tmp/probe-child-pid').write_text(str(child))
        time.sleep(30)
    elif case == 'overflow':
        print('x' * 5000)
    elif case == 'bad-version':
        print('Docker version unknown')
    elif case == 'stderr':
        print('unexpected', file=sys.stderr)
    else:
        print('Docker version 29.4.2, build abc123')
elif args[:3] == ['context', 'inspect', 'default']:
    print('tcp://example.invalid:2375' if case == 'remote' else 'unix:///var/run/docker.sock')
elif args[:4] == ['--host', 'unix:///run/docker.sock', 'info', '--format']:
    client = socket.socket(socket.AF_UNIX)
    client.connect('/run/docker.sock')
    client.close()
    if case == 'malformed':
        print('not json')
    elif case == 'extra-field':
        print(json.dumps({'ID':'fixture-id','ServerVersion':'29.4.2','OSType':'linux','Architecture':'x86_64','OperatingSystem':'Ubuntu 26.04','unexpected':'field'}))
    elif case == 'missing-field':
        print('{}')
    elif case == 'duplicate':
        print('{"ID":"one","ID":"two"}')
    elif case == 'wrong-platform':
        print(json.dumps({'ID':'fixture-id','ServerVersion':'29.4.2','OSType':'windows','Architecture':'x86_64','OperatingSystem':'fixture'}))
    else:
        print(json.dumps({'ID':'fixture-id','ServerVersion':'29.4.2','OSType':'linux','Architecture':'x86_64','OperatingSystem':'Ubuntu 26.04'}))
else:
    sys.exit(2)
'''

DRIVER = r'''import json, os, pathlib, shutil, socket, subprocess, sys, time
repo = pathlib.Path('/tmp/probe-repo')
script = repo / 'scripts/generator-release/observe-hosted-docker.py'
script.parent.mkdir(parents=True)
shutil.copyfile('/input/probe.py', script)
def command(*args):
    return subprocess.run(args, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout.strip()
command('git', '-C', str(repo), 'init', '-q')
command('git', '-C', str(repo), 'add', 'scripts')
command('git', '-C', str(repo), '-c', 'user.name=Alexey Zhokhov', '-c', 'user.email=alexey@zhokhov.com', '-c', 'commit.gpgsign=false', 'commit', '-qs', '-m', 'Test probe', '-m', 'Co-authored-by: Codex <codex@openai.com>')
sha = command('git', '-C', str(repo), 'rev-parse', 'HEAD')
env = {'PATH':'/usr/bin:/bin', 'GITHUB_EVENT_NAME':'workflow_dispatch', 'RUNNER_OS':'Linux', 'RUNNER_ARCH':'X64', 'ImageOS':'ubuntu26', 'ImageVersion':'20261101.186.1', 'GITHUB_WORKFLOW_SHA':sha, 'GITHUB_SHA':sha, 'GITHUB_REPOSITORY':'example/project', 'GITHUB_WORKFLOW_REF':'example/project/.github/workflows/source-qualification.yml@refs/heads/main', 'GITHUB_RUN_ID':'123', 'GITHUB_RUN_ATTEMPT':'1', 'GITHUB_WORKSPACE':str(repo), 'GITHUB_TOKEN':'secret-sentinel', 'DOCKER_HOST':'tcp://host.invalid:2375', 'DOCKER_CONTEXT':'bad-context', 'HTTP_PROXY':'http://proxy.invalid'}
case_path = pathlib.Path('/tmp/probe-case')
calls_path = pathlib.Path('/tmp/probe-calls')
sock_path = pathlib.Path('/run/docker.sock')
sock = socket.socket(socket.AF_UNIX)
sock.bind(str(sock_path))
sock.listen(10)
def run(case, expected, overrides=None):
    case_path.write_text(case)
    calls_path.unlink(missing_ok=True)
    result = subprocess.run(['/usr/bin/python3', '-I', str(script)], env={**env, **(overrides or {})}, capture_output=True, text=True, timeout=35)
    output = result.stdout + result.stderr
    assert 'secret-sentinel' not in output and 'proxy.invalid' not in output and 'host.invalid' not in output, case
    if expected is None:
        assert result.returncode == 0, (case, output)
        record = json.loads(result.stdout)
        assert record['authority'] == 'observation-only' and record['canonical_socket_path'] == '/run/docker.sock'
        assert record['observed_context_endpoint'] == 'unix:///var/run/docker.sock' and record['workflow_sha'] == sha
        calls = [json.loads(line) for line in calls_path.read_text().splitlines()]
        assert [call['args'][:4] for call in calls] == [['--version'], ['context','inspect','default','--format'], ['--host','unix:///run/docker.sock','info','--format']]
        for call in calls:
            assert set(call['env']) == {'PATH', 'HOME', 'DOCKER_CONFIG', 'LANG'}, call['env'].keys()
            assert call['env']['HOME'] == call['env']['DOCKER_CONFIG']
    else:
        assert result.returncode != 0 and expected in output, (case, result.returncode, output)
        if case in ('event', 'runner', 'ref', 'sha', 'checkout', 'source', 'socket'):
            assert not calls_path.exists(), case
run('good', None)
run('event', 'manual dispatch required', {'GITHUB_EVENT_NAME':'pull_request'})
run('runner', 'Ubuntu 26.04 x86-64 runner required', {'ImageOS':'ubuntu24'})
run('ref', 'invalid source or workflow identity', {'GITHUB_WORKFLOW_REF':'example/project/.github/workflows/other.yml@refs/heads/main'})
run('sha', 'invalid source or workflow identity', {'GITHUB_SHA':'b'*40})
run('checkout', 'checkout source identity mismatch', {'GITHUB_SHA':'b'*40, 'GITHUB_WORKFLOW_SHA':'b'*40})
run('bad-version', 'invalid Docker CLI identity')
run('remote', 'default Docker endpoint mismatch')
run('malformed', 'JSONDecodeError')
run('missing-field', 'invalid daemon identity')
run('extra-field', 'invalid daemon identity')
run('duplicate', 'duplicate daemon field')
run('wrong-platform', 'unsupported daemon identity')
run('overflow', 'Docker probe output exceeds bound')
run('stderr', 'invalid probe output')
run('timeout', 'Docker probe timed out')
pid = int(pathlib.Path('/tmp/probe-child-pid').read_text())
for _ in range(20):
    status = pathlib.Path('/proc') / str(pid) / 'stat'
    if not status.exists() or status.read_text().split()[2] == 'Z':
        break
    time.sleep(0.1)
else:
    raise AssertionError('Docker probe left descendant running')
assert not pathlib.Path('/tmp/probe-child-survived').exists()
script.write_text(script.read_text() + '\n')
run('source', 'probe source identity mismatch')
script.write_bytes(pathlib.Path('/input/probe.py').read_bytes())
sock.close()
sock_path.unlink()
sock_path.write_text('not a socket')
run('socket', 'invalid Docker socket')
print('Docker observation public-entrypoint tests passed')
'''


def main():
    with tempfile.TemporaryDirectory(prefix="velnor-docker-observation-test-") as directory:
        root = pathlib.Path(directory)
        fake = root / "docker"
        fake.write_text(FAKE_DOCKER)
        fake.chmod(0o755)
        driver = root / "driver.py"
        driver.write_text(DRIVER)
        subprocess.run([
            "docker", "run", "--rm", "--network", "none", "--platform", "linux/amd64",
            "--mount", f"type=bind,src={fake},dst=/usr/bin/docker,readonly",
            "--mount", f"type=bind,src={driver},dst=/driver.py,readonly",
            "--mount", f"type=bind,src={ROOT / 'scripts/generator-release/observe-hosted-docker.py'},dst=/input/probe.py,readonly",
            IMAGE, "python3", "-I", "/driver.py",
        ], check=True, timeout=120)


if __name__ == "__main__":
    main()

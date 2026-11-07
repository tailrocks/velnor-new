# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import hashlib
import json
import os
import re
import shutil
import stat
import struct
import sys
import tempfile
import time
import types


CONTEXT = (
    'GITHUB_REPOSITORY', 'GITHUB_REPOSITORY_ID', 'GITHUB_REPOSITORY_OWNER_ID',
    'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_JOB', 'GITHUB_SHA',
    'GITHUB_REF', 'GITHUB_EVENT_NAME', 'GITHUB_WORKFLOW_REF',
    'GITHUB_WORKFLOW_SHA', 'GITHUB_SERVER_URL', 'GITHUB_API_URL',
    'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_ARCH',
    'VELNOR_PREFLIGHT_ELIGIBLE', 'VELNOR_PREFLIGHT_SOURCE_SHA',
    'VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID', 'VELNOR_PREFLIGHT_RUST_ARTIFACT_ID',
)
OIDC_SECRETS = ('ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_URL')
MAX_BINARY = 100 * 1024 * 1024
BOOTSTRAP = '__VELNOR_BOOTSTRAP_PAYLOAD__'
MANAGER = '__VELNOR_MANAGER_PAYLOAD__'
EXPORT_CONTRACT = '__VELNOR_EXPORT_CONTRACT_PAYLOAD__'
EXPORT_VERIFIER = '__VELNOR_EXPORT_VERIFIER_PAYLOAD__'
WATCH_ADMISSION = '__VELNOR_WATCH_ADMISSION_PAYLOAD__'
MANAGER_POLICY_HOST = '__VELNOR_MANAGER_POLICY_HOST_PAYLOAD__'
MANAGER_POLICY_FILES = '__VELNOR_MANAGER_POLICY_FILES_PAYLOAD__'
MANAGER_POLICY = '__VELNOR_MANAGER_POLICY_PAYLOAD__'
LIFECYCLE = '__VELNOR_LIFECYCLE_PAYLOAD__'
HELPER = '__VELNOR_HELPER_PAYLOAD__'
SUPERVISOR = '__VELNOR_SUPERVISOR_PAYLOAD__'
WATCH_BOOTSTRAP = '__VELNOR_WATCH_BOOTSTRAP_PAYLOAD__'
WATCH_PROGRAM = '__VELNOR_WATCH_PROGRAM_PAYLOAD__'
CLAIM = '__VELNOR_CLAIM_PAYLOAD__'
WATCH_CONTRACT = 'velnor.root-watch.v1'
WATCH_OPERATION = 'publish'
JOB_TIMEOUT_SECONDS = int('__VELNOR_JOB_TIMEOUT_SECONDS__')
CLEANUP_RESERVE_NS = 25_000_000_000


def invalid():
    raise ValueError('publisher bootstrap rejected')


def valid_branch_ref(value):
    if not value.startswith('refs/heads/') or value.endswith(('/', '.')) or '..' in value or '@{' in value:
        return False
    if any(ord(char) < 33 or ord(char) == 127 or char in '~^:?*[\\' for char in value):
        return False
    return all(part and not part.startswith('.') and not part.endswith('.lock') for part in value.split('/'))


def read_context(mode):
    result = {}
    for name in CONTEXT + ('GH_TOKEN',):
        value = os.environ.get(name)
        if not value or len(value) > 8192 or any(ord(c) < 32 or ord(c) == 127 for c in value):
            invalid()
        result[name] = value
    credentials = OIDC_SECRETS if mode == 'trusted-publishing' else ('CARGO_REGISTRY_TOKEN',)
    for name in credentials:
        if name in os.environ:
            result[name] = os.environ[name]
    for name in ('GITHUB_REPOSITORY_ID', 'GITHUB_REPOSITORY_OWNER_ID', 'GITHUB_RUN_ID',
                 'GITHUB_RUN_ATTEMPT', 'VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID',
                 'VELNOR_PREFLIGHT_RUST_ARTIFACT_ID'):
        if not re.fullmatch(r'[1-9][0-9]{0,19}', result[name]) or int(result[name]) > 2**64 - 1:
            invalid()
    for name in ('GITHUB_SHA', 'GITHUB_WORKFLOW_SHA', 'VELNOR_PREFLIGHT_SOURCE_SHA'):
        if not re.fullmatch(r'[a-f0-9]{40}', result[name]):
            invalid()
    if (not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', result['GITHUB_REPOSITORY'])
            or result['GITHUB_JOB'] != ('release-publish' if mode == 'trusted-publishing' else 'release-publish-bootstrap')
            or result['GITHUB_RUN_ATTEMPT'] != '1'
            or not valid_branch_ref(result['GITHUB_REF'])
            or result['GITHUB_WORKFLOW_REF'] != result['GITHUB_REPOSITORY'] + '/__VELNOR_CRATE_RELEASE_WORKFLOW_PATH__@' + result['GITHUB_REF']
            or result['GITHUB_EVENT_NAME'] not in ('push', 'workflow_dispatch')
            or result['GITHUB_SERVER_URL'] != 'https://github.com'
            or result['GITHUB_API_URL'] != 'https://api.github.com'
            or result['RUNNER_ENVIRONMENT'] != 'github-hosted'
            or result['RUNNER_OS'] != 'Linux' or result['RUNNER_ARCH'] != 'X64'
            or result['VELNOR_PREFLIGHT_ELIGIBLE'] != 'true'
            or result['VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID'] == result['VELNOR_PREFLIGHT_RUST_ARTIFACT_ID']):
        invalid()
    return result


def bound_deadline(context, budget):
    if not isinstance(budget, str):
        invalid()
    parts = budget.split(':')
    if (len(parts) != 5 or parts[:4] != ['v1', context['GITHUB_RUN_ID'],
            context['GITHUB_RUN_ATTEMPT'], context['GITHUB_JOB']]
            or not re.fullmatch(r'[1-9][0-9]{0,19}', parts[4])):
        invalid()
    deadline = int(parts[4])
    now = time.clock_gettime_ns(time.CLOCK_BOOTTIME)
    if deadline <= now or deadline > now + JOB_TIMEOUT_SECONDS * 1_000_000_000:
        invalid()
    return deadline


def read_verified_binary(path, digest):
    if not re.fullmatch(r'[a-f0-9]{64}', digest):
        invalid()
    fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    try:
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                or info.st_mode & 0o077 or not info.st_mode & 0o100
                or info.st_size <= 0 or info.st_size > MAX_BINARY):
            invalid()
        digestor = hashlib.sha256()
        while True:
            part = os.read(fd, 1024 * 1024)
            if not part:
                break
            digestor.update(part)
        if digestor.hexdigest() != digest:
            invalid()
        os.lseek(fd, 0, os.SEEK_SET)
        return fd
    except BaseException:
        os.close(fd)
        raise


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            invalid()
        result[key] = value
    return result


def decoded(value):
    result = base64.b64decode(value, validate=True)
    if not result or len(result) > 512 * 1024 or b'\0' in result:
        invalid()
    return result


def module(name, payload):
    if payload == MANAGER:
        module('release_watch_admission', WATCH_ADMISSION)
        module('release_manager_policy_host', MANAGER_POLICY_HOST)
        module('release_manager_policy_files', MANAGER_POLICY_FILES)
        module('release_manager_policy', MANAGER_POLICY)
    result = types.ModuleType(name)
    sys.modules[name] = result
    try:
        exec(compile(decoded(payload), name, 'exec'), result.__dict__)
    except BaseException:
        del sys.modules[name]
        raise
    return result


def fixed_helper(root):
    content = decoded(HELPER)
    path = os.path.join(root, 'bootstrap-helper.py')
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW, 0o500)
    try:
        view = memoryview(content)
        while view:
            written = os.write(fd, view)
            if written <= 0:
                invalid()
            view = view[written:]
        os.fchmod(fd, 0o500)
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                or stat.S_IMODE(info.st_mode) != 0o500 or info.st_size != len(content)):
            invalid()
    finally:
        os.close(fd)
    return path


def stage_fixed(root, name, content, mode):
    if not re.fullmatch(r'release_[a-z_]+\.py', name) or mode not in (0o500, 0o555):
        invalid()
    path = os.path.join(root, name)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW, mode)
    try:
        view = memoryview(content)
        while view:
            written = os.write(fd, view)
            if written <= 0:
                invalid()
            view = view[written:]
        os.fchmod(fd, mode)
        info = os.fstat(fd)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                or stat.S_IMODE(info.st_mode) != mode or info.st_size != len(content)):
            invalid()
    finally:
        os.close(fd)
    return path


def acquire(root, script, env, deadline, watch):
    manager_module = module('velnor_root_manager', MANAGER)
    lifecycle = module('velnor_root_lifecycle', LIFECYCLE)
    helper = fixed_helper(root)
    body = json.dumps({'program': '/bin/bash', 'args': ['-euo', 'pipefail', '-c', script],
                       'cwd': root, 'env': env, 'stdin': ''}, separators=(',', ':')).encode('utf-8')
    if not 0 < len(body) <= 1024 * 1024:
        invalid()
    namespace = os.stat('/proc/self/ns/time').st_ino
    frame = struct.pack('>QQI', deadline, namespace, len(body)) + body
    manager = manager_module.Manager(helper, JOB_TIMEOUT_SECONDS, watch.authority, root_bootstrap=True)
    try:
        lifecycle.supervise(manager, frame, deadline, cleanup_reserve=CLEANUP_RESERVE_NS)
    finally:
        manager.close()


def prepare_binary(context, version, source_sha, manifest_sha, deadline, root, watch):
    script = decoded(BOOTSTRAP).decode('utf-8')
    run = os.stat('/run', follow_symlinks=False)
    if not stat.S_ISDIR(run.st_mode) or run.st_uid != 0 or run.st_mode & 0o022:
        invalid()
    info = os.stat(root, follow_symlinks=False)
    if info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o700:
        invalid()
    clean = {'PATH': '/usr/bin:/bin', 'HOME': root, 'TMPDIR': root, 'RUNNER_TEMP': root,
             'GH_TOKEN': context['GH_TOKEN'], 'VELNOR_GENERATOR_VERSION': version,
             'VELNOR_GENERATOR_SOURCE_SHA': source_sha, 'VELNOR_GENERATOR_MANIFEST_SHA256': manifest_sha}
    acquire(root, script, clean, deadline, watch)
    manifest = os.path.join(root, 'velnor', 'bootstrap', 'release-manifest.json')
    manifest_fd = os.open(manifest, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    with os.fdopen(manifest_fd, 'rb') as file:
        info = os.fstat(file.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                or info.st_mode & 0o077 or info.st_size > 1024 * 1024):
            invalid()
        data = file.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024 or hashlib.sha256(data).hexdigest() != manifest_sha:
        invalid()
    parsed = json.loads(data, object_pairs_hook=unique_object)
    targets = [item for item in parsed['targets'] if item.get('target') == 'x86_64-unknown-linux-gnu']
    if len(targets) != 1 or parsed.get('version') != version or parsed.get('commit') != source_sha:
        invalid()
    binary = os.path.join(root, 'velnor', 'bin', 'velnor-actions-' + version)
    digest = targets[0].get('sha256', '')
    fd = read_verified_binary(binary, digest)
    os.close(fd)
    return binary, digest


def main():
    if (os.geteuid() != 0 or sys.platform != 'linux' or len(sys.argv) != 5
            or os.uname().machine != 'x86_64' or not 60 < JOB_TIMEOUT_SECONDS <= 21600):
        invalid()
    mode, version, source_sha, manifest_sha = sys.argv[1:]
    if mode not in ('trusted-publishing', 'bootstrap'):
        invalid()
    context = read_context(mode)
    deadline = bound_deadline(context, os.environ.get("VELNOR_RELEASE_BUDGET"))
    if (not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version)
            or not re.fullmatch(r'[a-f0-9]{40}', source_sha)
            or not re.fullmatch(r'[a-f0-9]{64}', manifest_sha)):
        invalid()
    manager = module('velnor_publisher_manager', MANAGER)
    lifecycle = module('velnor_publisher_lifecycle', LIFECYCLE)
    watch_module = module('velnor_bootstrap_watch', WATCH_BOOTSTRAP)
    watch = watch_module.reclaim(manager, lifecycle, deadline, WATCH_OPERATION, decoded(WATCH_PROGRAM), decoded(CLAIM))
    root = None
    view = None
    completed = False
    try:
        root = tempfile.mkdtemp(prefix='velnor-publisher-', dir='/run')
        child = dict(context)
        child.update({'GIT_TOKEN': context['GH_TOKEN'], 'VELNOR_GENERATOR_VERSION': version,
                      'VELNOR_GENERATOR_SOURCE_SHA': source_sha, 'VELNOR_GENERATOR_MANIFEST_SHA256': manifest_sha})
        child.pop('GH_TOKEN')
        if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
            invalid()
        bundle = json.loads(decoded(SUPERVISOR), object_pairs_hook=unique_object)
        names = ('coordinator', 'retained', 'source', 'channel', 'upload', 'broker')
        if not isinstance(bundle, dict) or set(bundle) != set(names) or not all(isinstance(bundle[name], str) for name in names):
            invalid()
        binary, digest = prepare_binary(context, version, source_sha, manifest_sha, deadline, root, watch)
        view = tempfile.mkdtemp(prefix='velnor-publisher-view-', dir='/run')
        stage_fixed(root, 'release_coordinator_exec.py', decoded(bundle['coordinator']), 0o500)
        helper = decoded(HELPER)
        stage_fixed(view, 'release_supervisor_exec.py', helper, 0o555)
        retained = module('velnor_publisher_retained', bundle['retained'])
        source = module('velnor_publisher_source', bundle['source'])
        channel = module('velnor_publisher_channel', bundle['channel'])
        uploader = module('velnor_publisher_upload', bundle['upload'])
        broker = module('velnor_publisher_broker', bundle['broker'])
        retained.run(root, view, hashlib.sha256(helper).hexdigest(), version, digest, child,
                     deadline, manager, lifecycle, channel, source, uploader, broker, watch)
        completed = True
    finally:
        watch.close(deadline)
        if completed:
            shutil.rmtree(view)
            shutil.rmtree(root)


if __name__ == '__main__':
    try:
        main()
    except Exception:
        raise SystemExit('publisher bootstrap rejected') from None

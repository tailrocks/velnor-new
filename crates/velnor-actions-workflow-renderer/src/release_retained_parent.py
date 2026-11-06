# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import array
import hashlib
import json
import os
import queue
import re
import secrets
import socket
import stat
import struct
import threading
import time

COORDINATOR_CONTEXT = frozenset((
    'GITHUB_REPOSITORY', 'GITHUB_REPOSITORY_ID', 'GITHUB_REPOSITORY_OWNER_ID',
    'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT', 'GITHUB_JOB', 'GITHUB_SHA',
    'GITHUB_REF', 'GITHUB_EVENT_NAME', 'GITHUB_WORKFLOW_REF', 'GITHUB_WORKFLOW_SHA',
    'GITHUB_SERVER_URL', 'GITHUB_API_URL', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_ARCH',
    'VELNOR_PREFLIGHT_ELIGIBLE', 'VELNOR_PREFLIGHT_SOURCE_SHA',
    'VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID', 'VELNOR_PREFLIGHT_RUST_ARTIFACT_ID',
    'VELNOR_GENERATOR_VERSION', 'VELNOR_GENERATOR_SOURCE_SHA', 'VELNOR_GENERATOR_MANIFEST_SHA256',
    'GIT_TOKEN',
))


class ParentError(Exception):
    pass


def _remaining(deadline):
    value = deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
    if value <= 0:
        raise ParentError('publisher deadline')
    return value / 1_000_000_000


def _unit():
    return 'velnor-release-' + secrets.token_hex(16) + '.service'


def _stop(manager, lifecycle, owner, unit, deadline):
    job = manager.stop(unit, deadline)
    if not isinstance(job, str) or not re.fullmatch(r'/org/freedesktop/systemd1/job/[0-9]+', job):
        raise ParentError('publisher cleanup uncertain')
    lifecycle.wait_job(manager, unit, job, deadline)
    current = lifecycle.state(manager, owner, unit, deadline)
    if current['active'] not in ('inactive', 'failed') or current['job'] is not None or current['populated']:
        raise ParentError('publisher cleanup uncertain')
    manager.release(unit, deadline)


def _root_fd(root):
    if (not isinstance(root, str) or os.path.dirname(root) != '/run'
            or not re.fullmatch(r'velnor-publisher-[a-zA-Z0-9_-]+', os.path.basename(root))):
        raise ParentError('publisher root invalid')
    parent = os.open('/run', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(parent)
        if info.st_uid != 0 or info.st_mode & 0o022:
            raise ParentError('publisher root invalid')
        fd = os.open(os.path.basename(root), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                     dir_fd=parent)
    finally:
        os.close(parent)
    info = os.fstat(fd)
    if info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o700:
        os.close(fd)
        raise ParentError('publisher root invalid')
    return fd


def _view_helper(view, expected):
    if (not isinstance(view, str) or os.path.dirname(view) != '/run'
            or not re.fullmatch(r'velnor-publisher-view-[a-zA-Z0-9_-]+', os.path.basename(view))
            or not isinstance(expected, str) or not re.fullmatch(r'[a-f0-9]{64}', expected)):
        raise ParentError('source view invalid')
    parent = os.open('/run', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(parent)
        if info.st_uid != 0 or info.st_mode & 0o022:
            raise ParentError('source view invalid')
        directory = os.open(os.path.basename(view), os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                            dir_fd=parent)
    finally:
        os.close(parent)
    try:
        info = os.fstat(directory)
        if info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o700:
            raise ParentError('source view invalid')
        fd = os.open('release_supervisor_exec.py', os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=directory)
        try:
            info = os.fstat(fd)
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                    or stat.S_IMODE(info.st_mode) != 0o555 or not 0 < info.st_size <= 1024 * 1024):
                raise ParentError('source helper invalid')
            data = bytearray()
            while part := os.read(fd, 65536):
                data.extend(part)
                if len(data) > 1024 * 1024:
                    raise ParentError('source helper invalid')
            if len(data) != info.st_size or hashlib.sha256(data).hexdigest() != expected:
                raise ParentError('source helper invalid')
        finally:
            os.close(fd)
    except BaseException:
        os.close(directory)
        raise
    return view + '/release_supervisor_exec.py', directory


def run(root, view, helper_sha, version, digest, env, deadline, bus, lifecycle, channel, executor, uploader, broker, watch):
    if watch.data['operation'] != 'publish' or env.get('GITHUB_JOB') not in ('release-publish', 'release-publish-bootstrap'):
        raise ParentError('publisher role invalid')
    def serve(parent, source, locator, work_deadline, identity):
        def credentials(mode):
            if mode == 'trusted-publishing':
                return {'github_token': env.get('GIT_TOKEN'), 'oidc_token': env.get('ACTIONS_ID_TOKEN_REQUEST_TOKEN'),
                        'oidc_url': env.get('ACTIONS_ID_TOKEN_REQUEST_URL')}
            if mode == 'bootstrap-token':
                return {'github_token': env.get('GIT_TOKEN'), 'cargo_token': env.get('CARGO_REGISTRY_TOKEN')}
            raise ParentError('invalid upload mode')
        return channel.serve(parent, source, locator, lifecycle, executor, uploader, broker, credentials,
                             view, work_deadline, identity)
    return _run(root, view, helper_sha, version, digest, env, deadline, bus, lifecycle, channel,
                watch, 'release_coordinator_exec.py', serve)


def run_preflight(root, view, helper_sha, version, digest, env, deadline, bus, lifecycle, channel, executor, watch):
    if watch.data['operation'] != 'preflight' or env.get('GITHUB_JOB') not in ('release-preflight', 'release-reconcile'):
        raise ParentError('preflight role invalid')
    def serve(parent, source, locator, work_deadline, identity):
        return channel.serve_preflight(parent, source, locator, lifecycle, executor, view, work_deadline, identity)
    return _run(root, view, helper_sha, version, digest, env, deadline, bus, lifecycle, channel,
                watch, 'release_preflight_coordinator_exec.py', serve)


def _run(root, view, helper_sha, version, digest, env, deadline, bus, lifecycle, channel, watch, coordinator_name, serve):
    if os.geteuid() != 0 or type(deadline) is not int or _remaining(deadline) <= 25:
        raise ParentError('publisher unavailable')
    work_deadline = deadline - 25_000_000_000
    source_helper, view_fd = _view_helper(view, helper_sha)
    try:
        root_fd = _root_fd(root)
    except BaseException:
        os.close(view_fd)
        raise
    private_info, view_info = os.fstat(root_fd), os.fstat(view_fd)
    if (private_info.st_dev, private_info.st_ino) == (view_info.st_dev, view_info.st_ino):
        os.close(root_fd)
        os.close(view_fd)
        raise ParentError('publisher roots overlap')
    try:
        watch.manager.validate_watch(work_deadline)
        parent, child = socket.socketpair()
    except BaseException:
        os.close(root_fd)
        os.close(view_fd)
        raise
    started = queue.Queue(maxsize=1)
    finished = queue.Queue(maxsize=1)
    cancel = threading.Event()
    monitor = None
    source = None
    locator = None

    def coordinate():
        manager = None
        owner = None
        unit = _unit()
        attempted = False
        try:
            manager = bus.Manager(root + '/' + coordinator_name, int(_remaining(work_deadline)),
                                  watch=watch.authority, root_bootstrap=True)
            owner = manager.subscribe(work_deadline)
            attempted = True
            job = manager.start(unit, child.fileno(), work_deadline)
            child.close()
            lifecycle.wait_job(manager, unit, job, work_deadline)
            current = lifecycle.state(manager, owner, unit, work_deadline)
            invocation = current['invocation']
            if (not re.fullmatch(r'[0-9a-f]{32}', invocation)
                    or current['control_group'] != '/system.slice/' + unit
                    or current['active'] != 'active' or current['job'] is not None or not current['populated']):
                raise ParentError('coordinator identity unavailable')
            started.put_nowait(True)
            while not cancel.is_set():
                _remaining(work_deadline)
                current = lifecycle.state(manager, owner, unit, work_deadline)
                if current['invocation'] != invocation:
                    raise ParentError('coordinator identity changed')
                if current['active'] == 'inactive':
                    if (current['job'] is not None or current['populated'] or current['result'] != 'success'
                            or current['exec_code'] != 1 or current['exec_status'] != 0):
                        raise ParentError('coordinator completion unverified')
                    manager.release(unit, deadline)
                    finished.put_nowait(None)
                    return
                if current['active'] != 'active':
                    raise ParentError('coordinator failed')
                time.sleep(0.01)
            raise ParentError('coordinator cancelled')
        except BaseException as error:
            try:
                started.put_nowait(False)
            except queue.Full:
                pass
            try:
                parent.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            watch.end_liveness()
            if manager is not None and owner is not None and attempted:
                try:
                    _stop(manager, lifecycle, owner, unit, deadline)
                except Exception:
                    error = ParentError('coordinator cleanup uncertain')
            finished.put_nowait(error)
        finally:
            child.close()
            if manager is not None:
                manager.close()

    try:
        watch.manager.validate_watch(work_deadline)
        monitor = threading.Thread(target=coordinate)
        monitor.start()
        if not started.get(timeout=_remaining(work_deadline)):
            raise ParentError('coordinator unavailable')
        watch.manager.validate_watch(work_deadline)
        context = {key: value for key, value in env.items() if key in COORDINATOR_CONTEXT}
        body = json.dumps({'version': version, 'sha256': digest, 'env': context}, separators=(',', ':')).encode()
        if not 0 < len(body) <= 1024 * 1024:
            raise ParentError('coordinator frame invalid')
        parent.settimeout(_remaining(work_deadline))
        parent.sendall(struct.pack('>QQI', deadline, os.stat('/proc/self/ns/time').st_ino, len(body)) + body)
        if parent.sendmsg([b'R'], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array('i', [root_fd]))]) != 1:
            raise ParentError('publisher custody transfer failed')
        if parent.sendmsg([b'V'], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, array.array('i', [view_fd]))]) != 1:
            raise ParentError('publisher custody transfer failed')
        channel.ready(parent, work_deadline, False)
        source = bus.Manager(source_helper, int(_remaining(work_deadline)),
                             watch=watch.authority, deny_unix=True)
        source.subscribe(work_deadline)
        locator = bus.Manager(source_helper, int(_remaining(work_deadline)),
                              watch=watch.authority, deny_unix=True, deny_network=True)
        locator.subscribe(work_deadline)
        serve(parent, source, locator, work_deadline, (view_info.st_dev, view_info.st_ino))
        result = finished.get(timeout=_remaining(work_deadline))
        if result is not None:
            raise ParentError('coordinator failed') from result
    except BaseException:
        cancel.set()
        watch.end_liveness()
        try:
            parent.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        raise
    finally:
        cleanup_error = False
        for manager in (source, locator):
            if manager is not None:
                if manager.dropin_name is not None:
                    cleanup_error = True
                try:
                    manager.close()
                except Exception:
                    cleanup_error = True
        parent.close()
        child.close()
        if monitor is not None:
            remaining = max(0, deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)) / 1_000_000_000
            monitor.join(timeout=remaining)
            if monitor.is_alive():
                cleanup_error = True
        os.close(root_fd)
        os.close(view_fd)
        if cleanup_error:
            raise ParentError('publisher cleanup uncertain') from None

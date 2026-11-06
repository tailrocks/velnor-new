# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import hashlib
import json
import os
import re
import secrets
import shutil
import stat
import struct
import threading
import time


class WatchError(Exception):
    pass


def _fixed(root, name, content):
    if (os.path.dirname(root) != '/run'
            or not re.fullmatch(r'velnor-publisher-[a-zA-Z0-9_-]+', os.path.basename(root))
            or name not in ('release_launcher_watch.py', 'release_supervisor_claim.py')
            or type(content) is not bytes or not 0 < len(content) <= 1024 * 1024):
        raise WatchError('watch staging invalid')
    directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(directory)
        if info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o700:
            raise WatchError('watch staging invalid')
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o500, dir_fd=directory)
        try:
            view = memoryview(content)
            while view:
                size = os.write(fd, view)
                if size <= 0:
                    raise WatchError('watch staging incomplete')
                view = view[size:]
            os.fchmod(fd, 0o500)
        finally:
            os.close(fd)
    finally:
        os.close(directory)
    return root + '/' + name


def _empty(unit):
    if not re.fullmatch(r'velnor-release-[a-f0-9]{32}\.service', unit):
        raise WatchError('watch dependent invalid')
    try:
        with open('/sys/fs/cgroup/system.slice/' + unit + '/cgroup.events', encoding='ascii') as stream:
            data = stream.read(4097)
    except FileNotFoundError:
        return True
    matches = re.findall(r'^populated ([01])$', data, re.MULTILINE)
    if len(data) > 4096 or len(matches) != 1:
        raise WatchError('watch dependent state invalid')
    return matches[0] == '0'


class Watch:
    def __init__(self, bus, lifecycle, manager, authority, data):
        self.bus = bus
        self.lifecycle = lifecycle
        self.manager = manager
        self.authority = authority
        self.data = data
        self.closed = False
        self.writer_open = True
        self.lock = threading.Lock()

    def end_liveness(self):
        with self.lock:
            if self.writer_open:
                self.writer_open = False
                os.close(self.authority[1])

    def prepare_exec_environment(self):
        if self.closed or not self.writer_open:
            raise WatchError('watch closed')
        self.manager.validate_watch(self.data['deadline'])
        lease, writer, namespace, _operation = self.authority
        descriptor = self.manager._check(self.manager.library.sd_bus_get_fd(self.manager.bus))
        os.set_inheritable(descriptor, False)
        os.set_inheritable(lease, True)
        os.set_inheritable(writer, True)
        return {'VELNOR_RELEASE_WATCH_LEASE_FD': str(lease), 'VELNOR_RELEASE_WATCH_WRITER_FD': str(writer),
                'VELNOR_RELEASE_WATCH_NAMESPACE': namespace}

    def close(self, deadline):
        if self.closed:
            raise WatchError('watch closed twice')
        self.closed = True
        unit = self.data['unit']
        dependents = set()
        error = None
        try:
            try:
                self.bus.watch_staging(self.data)
                dependents.update(self.manager.bound_units(unit, deadline))
            finally:
                self.end_liveness()
            job = self.manager.stop(unit, deadline)
            self.lifecycle.wait_job(self.manager, unit, job, deadline)
            while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < deadline:
                current = self.lifecycle.state(self.manager, self.manager.owner, unit, deadline)
                if current['invocation'] != self.data['invocation']:
                    raise WatchError('watch invocation changed during cleanup')
                dependents.update(self.manager.bound_units(unit, deadline))
                if (current['active'] in ('inactive', 'failed') and current['job'] is None
                        and not current['populated'] and all(_empty(item) for item in dependents)):
                    self.manager.release(unit, deadline)
                    shutil.rmtree(self.data['staging_root'])
                    return
                time.sleep(0.01)
            raise WatchError('watch cleanup deadline')
        except BaseException as failure:
            error = failure
            raise
        finally:
            os.close(self.authority[0])
            try:
                self.manager.close()
            except BaseException:
                if error is None:
                    raise


def start(bus, lifecycle, root, deadline, operation, program, claim):
    remaining = (deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)) // 1_000_000_000
    if os.geteuid() != 0 or operation not in ('publish', 'preflight') or remaining <= 25:
        raise WatchError('watch unavailable')
    namespace = secrets.token_hex(16)
    unit = 'velnor-release-' + namespace + '.service'
    helper = _fixed(root, 'release_launcher_watch.py', program)
    claim_path = _fixed(root, 'release_supervisor_claim.py', claim)
    program_sha = hashlib.sha256(program).hexdigest()
    claim_sha = hashlib.sha256(claim).hexdigest()
    manager = bus.Manager.watch_manager(helper, claim_path, remaining, unit, program_sha, claim_sha)
    try:
        reader, writer = os.pipe2(os.O_CLOEXEC)
    except BaseException:
        manager.close()
        raise
    lease = None
    attempted = False
    try:
        os.fchmod(writer, 0o600)
        time_namespace = os.stat('/proc/self/ns/time').st_ino
        if os.write(writer, struct.pack('>QQ', deadline, time_namespace)) != 16:
            raise WatchError('watch frame incomplete')
        owner = manager.subscribe(deadline)
        attempted = True
        job = manager.start(unit, reader, deadline)
        lifecycle.wait_job(manager, unit, job, deadline)
        current = lifecycle.state(manager, owner, unit, deadline)
        if (not re.fullmatch(r'[a-f0-9]{32}', current['invocation'])
                or current['active'] != 'active' or current['job'] is not None or not current['populated']
                or current['control_group'] != '/system.slice/' + unit):
            raise WatchError('watch identity unavailable')
        pipe = os.fstat(writer)
        with open('/proc/self/stat', encoding='ascii') as stream:
            root_start = stream.read(4096).rsplit(')', 1)[1].split()[19]
        root_info = os.stat(root, follow_symlinks=False)
        data = {'staging_root': root, 'staging_device': root_info.st_dev, 'staging_inode': root_info.st_ino,
                'schema': 'velnor.root-watch.v1', 'unit': unit, 'invocation': current['invocation'],
                'deadline': deadline, 'time_namespace': time_namespace, 'namespace': namespace, 'operation': operation,
                'writer_device': pipe.st_dev, 'writer_inode': pipe.st_ino, 'root_pid': os.getpid(), 'root_start': root_start,
                'program_sha256': program_sha, 'claim_sha256': claim_sha, 'runtime_seconds': manager._watch_runtime_seconds}
        raw = json.dumps(data, sort_keys=True, separators=(',', ':')).encode()
        lease = os.memfd_create('velnor-root-watch', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
        os.fchmod(lease, 0o600)
        if not 0 < len(raw) <= 4096 or os.write(lease, raw) != len(raw):
            raise WatchError('watch lease incomplete')
        fcntl.fcntl(lease, fcntl.F_ADD_SEALS,
                    fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
        authority = (lease, writer, namespace, operation)
        manager.watch = authority
        manager._watch_data = bus.read_watch(authority)
        manager._watch_start_allowed = False
        manager.validate_watch(deadline)
        return Watch(bus, lifecycle, manager, authority, data)
    except BaseException:
        os.close(writer)
        if lease is not None:
            os.close(lease)
        try:
            if attempted:
                job = manager.stop(unit, deadline)
                lifecycle.wait_job(manager, unit, job, deadline)
                current = lifecycle.state(manager, manager.owner, unit, deadline)
                if current['active'] not in ('inactive', 'failed') or current['job'] is not None or current['populated']:
                    raise WatchError('watch startup cleanup uncertain') from None
                manager.release(unit, deadline)
        finally:
            manager.close()
        raise
    finally:
        os.close(reader)


def reclaim(bus, lifecycle, deadline, operation, program, claim):
    names = ('VELNOR_RELEASE_WATCH_LEASE_FD', 'VELNOR_RELEASE_WATCH_WRITER_FD', 'VELNOR_RELEASE_WATCH_NAMESPACE')
    values = [os.environ.get(name) for name in names]
    if (not all(isinstance(value, str) for value in values)
            or not all(re.fullmatch(r'[1-9][0-9]{0,9}', value) for value in values[:2])):
        raise WatchError('watch handoff unavailable')
    authority = (int(values[0]), int(values[1]), values[2], operation)
    for fd in authority[:2]:
        os.set_inheritable(fd, False)
    data = bus.read_watch(authority)
    if (data['deadline'] != deadline or data['program_sha256'] != hashlib.sha256(program).hexdigest()
            or data['claim_sha256'] != hashlib.sha256(claim).hexdigest()):
        raise WatchError('watch handoff changed')
    manager = bus.Manager.reclaim_watch(authority, deadline)
    return Watch(bus, lifecycle, manager, authority, data)

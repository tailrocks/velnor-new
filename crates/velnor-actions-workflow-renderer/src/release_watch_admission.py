# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import fcntl
import hashlib
import json
import os
import re
import stat
import time


class ManagerError(OSError):
    pass


def read_watch(authority):
    if os.geteuid() != 0 or type(authority) is not tuple or len(authority) != 4:
        raise ManagerError("watch authority unavailable")
    lease, writer, namespace, operation = authority
    if (type(lease) is not int or type(writer) is not int or lease < 3 or writer < 3 or lease == writer
            or not isinstance(namespace, str) or not re.fullmatch(r"[a-f0-9]{32}", namespace)
            or operation not in ("publish", "preflight")):
        raise ManagerError("watch authority invalid")
    metadata = os.fstat(lease)
    seals = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
    if (not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_nlink != 0
            or stat.S_IMODE(metadata.st_mode) != 0o600 or not 0 < metadata.st_size <= 4096
            or fcntl.fcntl(lease, fcntl.F_GET_SEALS) & seals != seals):
        raise ManagerError("watch lease invalid")
    raw = os.pread(lease, metadata.st_size + 1, 0)
    def unique(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                raise ManagerError("watch lease duplicate")
            value[key] = item
        return value
    data = json.loads(raw, object_pairs_hook=unique)
    fields = {"schema", "unit", "invocation", "deadline", "time_namespace", "namespace", "operation",
              "staging_root", "staging_device", "staging_inode",
              "writer_device", "writer_inode", "root_pid", "root_start", "program_sha256", "claim_sha256", "runtime_seconds"}
    pipe = os.fstat(writer)
    integers = ('deadline', 'time_namespace', 'writer_device', 'writer_inode', 'root_pid', 'runtime_seconds', 'staging_device', 'staging_inode')
    with open('/proc/self/stat', encoding='ascii') as stream:
        start = stream.read(4096).rsplit(')', 1)[1].split()[19]
    if (type(data) is not dict or set(data) != fields
            or json.dumps(data, sort_keys=True, separators=(',', ':')).encode() != raw
            or any(type(data[key]) is not int or not 0 < data[key] < 2**64 for key in integers)
            or os.get_inheritable(lease) or os.get_inheritable(writer)
            or data['schema'] != 'velnor.root-watch.v1' or data['namespace'] != namespace
            or data['unit'] != 'velnor-release-' + namespace + '.service' or data['operation'] != operation
            or type(data['deadline']) is not int or data['deadline'] <= time.clock_gettime_ns(time.CLOCK_BOOTTIME)
            or data['time_namespace'] != os.stat('/proc/self/ns/time').st_ino
            or data['root_pid'] != os.getpid() or data['root_start'] != start
            or not isinstance(data['invocation'], str) or not re.fullmatch(r'[a-f0-9]{32}', data['invocation'])
            or any(not isinstance(data[key], str) or not re.fullmatch(r'[a-f0-9]{64}', data[key])
                   for key in ('program_sha256', 'claim_sha256'))
            or type(data['runtime_seconds']) is not int or not 1 <= data['runtime_seconds'] <= 86400
            or not stat.S_ISFIFO(pipe.st_mode) or pipe.st_uid != 0 or stat.S_IMODE(pipe.st_mode) != 0o600
            or (pipe.st_dev, pipe.st_ino) != (data['writer_device'], data['writer_inode'])
            or fcntl.fcntl(writer, fcntl.F_GETFL) & os.O_ACCMODE != os.O_WRONLY):
        raise ManagerError("watch authority changed")
    watch_staging(data)
    return data


def watch_staging(data):
    directory = data['staging_root']
    if (not isinstance(directory, str) or os.path.dirname(directory) != '/run'
            or not re.fullmatch(r'velnor-publisher-[a-zA-Z0-9_-]+', os.path.basename(directory))):
        raise ManagerError('watch staging identity invalid')
    fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(fd)
        if (info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o700
                or (info.st_dev, info.st_ino) != (data['staging_device'], data['staging_inode'])):
            raise ManagerError('watch staging identity changed')
    finally:
        os.close(fd)



def verify_programs(helper, claim_helper, unit, program_sha256, claim_sha256):
    if (os.geteuid() != 0 or not re.fullmatch(r"velnor-release-[0-9a-f]{32}\.service", unit)
            or os.path.basename(helper) != 'release_launcher_watch.py'
            or os.path.basename(claim_helper) != 'release_supervisor_claim.py'):
        raise ManagerError("watch factory invalid")
    directory = os.path.dirname(helper)
    if (directory != os.path.dirname(claim_helper) or os.path.dirname(directory) != '/run'
            or not re.fullmatch(r'velnor-publisher-[a-zA-Z0-9_-]+', os.path.basename(directory))):
        raise ManagerError('watch program parent invalid')
    parent = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(parent)
        if info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o700:
            raise ManagerError('watch program parent invalid')
    finally:
        os.close(parent)
    for path, expected in ((helper, program_sha256), (claim_helper, claim_sha256)):
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            info = os.fstat(fd)
            data = os.read(fd, 1024 * 1024 + 1)
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                    or stat.S_IMODE(info.st_mode) != 0o500 or not 0 < info.st_size <= 1024 * 1024
                    or len(data) != info.st_size or hashlib.sha256(data).hexdigest() != expected):
                raise ManagerError("watch program invalid")
        finally:
            os.close(fd)


def open_dropin(data):
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    parent = os.open('/run/systemd/system', flags)
    directory = descriptor = None
    try:
        info = os.fstat(parent)
        if info.st_uid != 0 or info.st_mode & 0o022:
            raise ManagerError('watch staging invalid')
        directory = os.open(data['unit'] + '.d', flags, dir_fd=parent)
        descriptor = os.open('deadline.conf', os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=directory)
        expected = ('[Unit]\nJobTimeoutSec=' + str(data['runtime_seconds']) + 's\n').encode('ascii')
        result = parent, data['unit'] + '.d', directory, descriptor, expected
        parent = directory = descriptor = None
        return result
    finally:
        for held in (descriptor, directory, parent):
            if held is not None:
                os.close(held)

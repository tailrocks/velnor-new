# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
import stat
import time

from release_watch_admission import ManagerError


def check(deadline):
    if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
        raise ManagerError('manager policy deadline')


def identity(path):
    value = os.stat(path)
    return value.st_dev, value.st_ino


def digest(descriptor, maximum, deadline):
    before = os.fstat(descriptor)
    if (not stat.S_ISREG(before.st_mode) or before.st_uid != 0
            or before.st_mode & 0o022 or before.st_nlink != 1
            or not 0 < before.st_size <= maximum):
        raise ManagerError('untrusted manager policy file')
    value = hashlib.sha256()
    total = 0
    while data := os.read(descriptor, 65536):
        check(deadline)
        total += len(data)
        if total > maximum:
            raise ManagerError('manager policy file exceeds bound')
        value.update(data)
    after = os.fstat(descriptor)
    shape = lambda entry: (entry.st_dev, entry.st_ino, entry.st_uid, entry.st_mode,
                           entry.st_nlink, entry.st_size, entry.st_mtime_ns, entry.st_ctime_ns)
    if shape(before) != shape(after) or total != before.st_size:
        raise ManagerError('manager policy file changed')
    check(deadline)
    return value.hexdigest(), shape(before)


def executable_mappings(base, deadline):
    descriptor = os.open(base + '/maps', os.O_RDONLY | os.O_CLOEXEC)
    try:
        chunks = []
        total = 0
        while chunk := os.read(descriptor, 65536):
            check(deadline)
            total += len(chunk)
            if total > 512 * 1024:
                raise ManagerError('manager executable mapping bound')
            chunks.append(chunk)
        data = b''.join(chunks)
    finally:
        os.close(descriptor)
    if not data or len(data) > 512 * 1024:
        raise ManagerError('manager executable mapping bound')
    rows = data.decode('utf-8', errors='strict').splitlines()
    if len(rows) > 4096:
        raise ManagerError('manager executable mapping count')
    mappings = {}
    for row in rows:
        check(deadline)
        fields = row.split(None, 5)
        if len(fields) < 2 or 'x' not in fields[1]:
            continue
        name = fields[5] if len(fields) == 6 else ''
        if name in ('[vdso]', '[vsyscall]'):
            continue
        if (not name.startswith('/') or name.endswith(' (deleted)') or '\\' in name
                or any(part in ('', '.', '..') for part in name[1:].split('/'))):
            raise ManagerError('unbound manager executable mapping')
        descriptor = os.open(base + '/root' + name, os.O_RDONLY | os.O_CLOEXEC)
        try:
            value = digest(descriptor, 256 * 1024 * 1024, deadline)
            major, minor = (int(part, 16) for part in fields[3].split(':'))
            if value[1][0:2] != (os.makedev(major, minor), int(fields[4])):
                raise ManagerError('manager mapped executable identity mismatch')
            if name in mappings and mappings[name] != value:
                raise ManagerError('manager executable mapping changed')
            mappings[name] = value
        finally:
            os.close(descriptor)
    if not 0 < len(mappings) <= 128:
        raise ManagerError('manager executable inventory invalid')
    return tuple(sorted(mappings.items()))


def manager_environment(manager, path, interface, deadline):
    values = manager._get(path, interface, 'Environment', 'as', deadline)
    if (type(values) is not list or len(values) > 256
            or any(type(value) is not str for value in values)
            or sum(len(value.encode('utf-8')) for value in values) > 65536):
        raise ManagerError('manager environment bound')
    names = []
    for value in values:
        name, separator, value = value.partition('=')
        if (not separator or not name or not name.isascii()
                or not name.replace('_', 'a').isalnum() or name in names):
            raise ManagerError('manager environment name invalid')
        names.append(name)
    encoded = b'\0'.join(value.encode('utf-8') for value in values)
    return tuple(sorted(names)), hashlib.sha256(encoded).digest()


def process(pid, deadline):
    check(deadline)
    base = '/proc/' + str(pid)
    descriptor = os.open(base + '/stat', os.O_RDONLY | os.O_CLOEXEC)
    try:
        data = os.read(descriptor, 4097)
        if not 0 < len(data) <= 4096:
            raise ManagerError('manager process identity invalid')
        start = data.rsplit(b')', 1)[1].split()[19]
    finally:
        os.close(descriptor)
    root = identity(base + '/root')
    namespaces = tuple(identity(base + '/ns/' + name) for name in ('pid', 'mnt', 'time'))
    if root != identity('/proc/self/root') or namespaces != tuple(
            identity('/proc/self/ns/' + name) for name in ('pid', 'mnt', 'time')):
        raise ManagerError('manager and controller filesystem views differ')
    descriptor = os.open(base + '/exe', os.O_RDONLY | os.O_CLOEXEC)
    try:
        executable = digest(descriptor, 256 * 1024 * 1024, deadline)
    finally:
        os.close(descriptor)
    return (pid, start, root, namespaces, executable,
            executable_mappings(base, deadline), os.readlink(base + '/exe'))


class Host:
    def __init__(self, manager, deadline):
        self.owner = manager.owner
        self.facts = self.observe(manager, deadline)

    def observe(self, manager, deadline):
        check(deadline)
        if manager._owner(deadline) != self.owner:
            raise ManagerError('manager policy owner changed')
        pid = manager._method('org.freedesktop.DBus', '/org/freedesktop/DBus',
                              'org.freedesktop.DBus', 'GetConnectionUnixProcessID',
                              [('s', self.owner)], deadline, 'u')
        uid = manager._method('org.freedesktop.DBus', '/org/freedesktop/DBus',
                              'org.freedesktop.DBus', 'GetConnectionUnixUser',
                              [('s', self.owner)], deadline, 'u')
        if uid != 0 or type(pid) is not int or pid <= 0:
            raise ManagerError('untrusted manager policy owner')
        path, interface = '/org/freedesktop/systemd1', 'org.freedesktop.systemd1.Manager'
        version = manager._get(path, interface, 'Version', 's', deadline)
        roots = manager._get(path, interface, 'UnitPath', 'as', deadline)
        if (type(version) is not str or type(roots) is not list or not 0 < len(roots) <= 64
                or len(set(roots)) != len(roots)
                or any(type(root) is not str or not root.startswith('/')
                       or root.endswith('/') or any(part in ('', '.', '..')
                       for part in root[1:].split('/')) for root in roots)):
            raise ManagerError('invalid authoritative manager policy roots')
        facts = (process(pid, deadline), version, tuple(roots),
                 manager_environment(manager, path, interface, deadline))
        if manager._owner(deadline) != self.owner:
            raise ManagerError('manager policy owner changed')
        return facts

    @property
    def environment_names(self):
        return self.facts[3][0]

    def revalidate(self, manager, deadline):
        if self.observe(manager, deadline) != self.facts:
            raise ManagerError('manager policy identity changed')

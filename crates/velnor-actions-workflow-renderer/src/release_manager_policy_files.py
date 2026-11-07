# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import stat

from release_manager_policy_host import check, digest
from release_watch_admission import ManagerError


def shape(info):
    return info.st_dev, info.st_ino, info.st_uid, info.st_gid, info.st_mode


class Files:
    def __init__(self, profile, deadline):
        self.profile = profile
        self.deadline = deadline
        self.descriptors = []
        self.directories = {}
        self.proofs = []
        self.listings = []
        self.files = []
        try:
            descriptor = self.keep(os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC))
            self.trusted(os.fstat(descriptor))
            self.directories['/'] = descriptor
        except BaseException:
            self.close()
            raise

    def keep(self, descriptor):
        self.descriptors.append(descriptor)
        return descriptor

    def trusted(self, info):
        check(self.deadline)
        if info.st_uid != 0 or info.st_mode & 0o022 or not stat.S_ISDIR(info.st_mode):
            raise ManagerError('untrusted manager policy ancestor')

    def observe(self, parent, name):
        try:
            info = os.stat(name, dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError:
            info = None
        target = os.readlink(name, dir_fd=parent) if info and stat.S_ISLNK(info.st_mode) else None
        self.proofs.append((parent, name, None if info is None else shape(info), target))
        return info, target

    def directory(self, path, depth=0):
        check(self.deadline)
        if depth > 4:
            raise ManagerError('manager policy ancestor link depth')
        if path in self.directories:
            return self.directories[path]
        parent, name = path.rsplit('/', 1)
        parent = self.directory(parent or '/', depth)
        if parent is None:
            self.directories[path] = None
            return None
        info, target = self.observe(parent, name)
        if info is None:
            self.directories[path] = None
            return None
        if path in self.profile['symlinks'] and target is None:
            raise ManagerError('reviewed manager ancestor link replaced')
        if target is not None:
            if info.st_uid != 0 or self.profile['symlinks'].get(path) != target:
                raise ManagerError('unreviewed manager policy ancestor link')
            if target.startswith('/'):
                resolved = target
            else:
                resolved = (path.rsplit('/', 1)[0] + '/' + target).replace('//', '/')
            if any(part in ('', '.', '..') for part in resolved[1:].split('/')):
                raise ManagerError('invalid manager policy ancestor link')
            descriptor = self.directory(resolved, depth + 1)
        else:
            self.trusted(info)
            descriptor = self.keep(os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
                                           | os.O_CLOEXEC, dir_fd=parent))
            if shape(os.fstat(descriptor)) != shape(info):
                raise ManagerError('manager policy ancestor replaced')
        self.directories[path] = descriptor
        return descriptor

    def names(self, descriptor):
        entries = []
        with os.scandir(descriptor) as iterator:
            for entry in iterator:
                check(self.deadline)
                if len(entries) >= 4096:
                    raise ManagerError('manager policy scope exceeds bound')
                entries.append(entry.name)
        entries.sort()
        return entries

    def listing(self, descriptor):
        entries = self.names(descriptor)
        self.listings.append((descriptor, tuple(entries)))
        return entries

    def file(self, parent, name, expected, generated=None, maximum=1024 * 1024):
        descriptor = self.keep(os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC,
                                       dir_fd=parent))
        observed, metadata = digest(descriptor, maximum, self.deadline)
        if observed != expected:
            raise ManagerError('unreviewed manager policy bytes')
        if generated is not None and (
                shape(os.fstat(descriptor)) != generated.file_identity
                or shape(os.fstat(generated.descriptor)) != generated.file_identity):
            raise ManagerError('generated deadline file replaced')
        self.files.append((descriptor, parent, name, metadata, maximum))

    def code(self, path, expected):
        parent, name = path.rsplit('/', 1)
        directory = self.directory(parent)
        if directory is None:
            raise ManagerError('manager executable ancestor missing')
        info, target = self.observe(directory, name)
        if info is None or target is not None or not stat.S_ISREG(info.st_mode):
            raise ManagerError('manager executable path changed')
        self.file(directory, name, expected, maximum=256 * 1024 * 1024)

    def scope(self, root, scope, generated):
        path = root + '/' + scope
        expected = self.profile['policy'].get(path)
        parent = self.directory(root)
        if parent is None:
            if expected is not None:
                raise ManagerError('reviewed manager policy root missing')
            return
        info, target = self.observe(parent, scope)
        if info is None:
            if expected is not None:
                raise ManagerError('reviewed manager policy missing')
            return
        own = path == generated.path
        if target is not None or expected is None and not own:
            raise ManagerError('unreviewed applicable manager policy')
        if own:
            if (shape(info) != generated.directory_identity
                    or shape(os.fstat(generated.directory)) != generated.directory_identity):
                raise ManagerError('generated deadline directory replaced')
        elif info.st_uid != 0 or stat.S_IMODE(info.st_mode) != expected['mode']:
            raise ManagerError('manager policy permissions changed')
        if stat.S_ISREG(info.st_mode):
            if expected['kind'] != 'file':
                raise ManagerError('manager policy kind changed')
            self.file(parent, scope, expected['sha256'])
        elif stat.S_ISDIR(info.st_mode):
            if not own and expected['kind'] != 'directory':
                raise ManagerError('manager policy kind changed')
            directory = self.directory(path)
            entries = self.listing(directory)
            expected_names = ['deadline.conf'] if own else sorted(
                item[len(path) + 1:] for item in self.profile['policy']
                if item.startswith(path + '/') and '/' not in item[len(path) + 1:])
            if entries != expected_names:
                raise ManagerError('reviewed manager policy scope changed')
            for name in entries:
                child = path + '/' + name
                item = self.profile['policy'].get(child)
                if own:
                    if name != 'deadline.conf':
                        raise ManagerError('unexpected generated deadline policy')
                    self.file(directory, name, generated.sha256, generated)
                elif item is None or item['kind'] != 'file':
                    raise ManagerError('unreviewed manager policy scope entry')
                else:
                    child_info, child_target = self.observe(directory, name)
                    if (child_info is None or child_target is not None
                            or stat.S_IMODE(child_info.st_mode) != item['mode']):
                        raise ManagerError('manager policy scope entry changed')
                    self.file(directory, name, item['sha256'])
        else:
            raise ManagerError('unsupported manager policy entry')

    def revalidate(self):
        check(self.deadline)
        for parent, name, before, target in self.proofs:
            try:
                info = os.stat(name, dir_fd=parent, follow_symlinks=False)
            except FileNotFoundError:
                info = None
            if (None if info is None else shape(info)) != before:
                raise ManagerError('manager policy path changed')
            if target is not None and os.readlink(name, dir_fd=parent) != target:
                raise ManagerError('manager policy link changed')
        for descriptor, entries in self.listings:
            observed = tuple(self.names(descriptor))
            if observed != entries:
                raise ManagerError('manager policy scope changed')
        for descriptor, parent, name, before, maximum in self.files:
            os.lseek(descriptor, 0, os.SEEK_SET)
            _, after = digest(descriptor, maximum, self.deadline)
            if before != after or shape(os.stat(name, dir_fd=parent, follow_symlinks=False)) != shape(os.fstat(descriptor)):
                raise ManagerError('manager policy file changed')
        check(self.deadline)

    def close(self):
        for descriptor in self.descriptors:
            os.close(descriptor)
        self.descriptors.clear()

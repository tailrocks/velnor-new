"""Cold admission until authenticated producer inventories are qualified.

No writable marker grants executable authority. Retain only the separately
qualified Mise binary; discard every other restored entry before installation.
"""
import hashlib
import os
import stat


def open_root(path, boundary, allow_boundary=False):
    parts = path.split('/')
    boundary_parts = boundary.split('/')
    if (not path.startswith('/') or not boundary.startswith('/')
            or any(part in ('', '.', '..') for part in parts[1:] + boundary_parts[1:])
            or len(parts) < len(boundary_parts)
            or (not allow_boundary and len(parts) == len(boundary_parts))
            or parts[:len(boundary_parts)] != boundary_parts):
        raise ValueError('native_health_root')
    descriptor = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    try:
        for depth, part in enumerate(parts[1:], start=1):
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
            if depth >= len(boundary_parts) - 1:
                info = os.fstat(descriptor)
                if info.st_uid != os.geteuid() or info.st_mode & 0o022:
                    raise ValueError('native_health_owner')
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def verify_manager(directory, expected):
    if len(expected) != 64 or any(char not in '0123456789abcdef' for char in expected):
        raise ValueError('native_health_manager_binding')
    descriptor = os.open('mise', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                         dir_fd=directory)
    try:
        info = os.fstat(descriptor)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                or info.st_nlink != 1 or info.st_mode & 0o022
                or not info.st_mode & 0o100 or info.st_size > executable_limit()):
            raise ValueError('native_health_manager_shape')
        with os.fdopen(os.dup(descriptor), 'rb') as stream:
            actual = stream_sha256(stream)
        if actual != expected:
            raise ValueError('native_health_manager_digest')
    finally:
        os.close(descriptor)


def _require_same_device(info, device):
    if info.st_dev != device:
        raise ValueError('native_health_entry_device')


def _preflight_entry(directory, name, device):
    info = os.stat(name, dir_fd=directory, follow_symlinks=False)
    _require_same_device(info, device)
    if not stat.S_ISDIR(info.st_mode):
        return
    child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                    dir_fd=directory)
    try:
        current = os.fstat(child)
        _require_same_device(current, device)
        if (current.st_dev, current.st_ino) != (info.st_dev, info.st_ino):
            raise ValueError('native_health_entry_changed')
        for entry in os.listdir(child):
            _preflight_entry(child, entry, device)
        final = os.stat(name, dir_fd=directory, follow_symlinks=False)
        _require_same_device(final, device)
        if (final.st_dev, final.st_ino) != (info.st_dev, info.st_ino):
            raise ValueError('native_health_entry_changed')
    finally:
        os.close(child)


def _delete_entry_checked(directory, name, device):
    info = os.stat(name, dir_fd=directory, follow_symlinks=False)
    _require_same_device(info, device)
    if not stat.S_ISDIR(info.st_mode):
        os.unlink(name, dir_fd=directory)
        return
    child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                    dir_fd=directory)
    try:
        current = os.fstat(child)
        _require_same_device(current, device)
        if (current.st_dev, current.st_ino) != (info.st_dev, info.st_ino):
            raise ValueError('native_health_entry_changed')
        for entry in os.listdir(child):
            _delete_entry_checked(child, entry, device)
        final = os.stat(name, dir_fd=directory, follow_symlinks=False)
        _require_same_device(final, device)
        if (final.st_dev, final.st_ino) != (info.st_dev, info.st_ino):
            raise ValueError('native_health_entry_changed')
        os.rmdir(name, dir_fd=directory)
    finally:
        os.close(child)


def remove_entry(directory, name):
    device = os.fstat(directory).st_dev
    _preflight_entry(directory, name, device)
    _delete_entry_checked(directory, name, device)


def cold_prepare(path, expected, boundary):
    root = open_root(path, boundary)
    try:
        binary = os.open('bin', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                         dir_fd=root)
        try:
            info = os.fstat(binary)
            if info.st_uid != os.geteuid() or info.st_mode & 0o022:
                raise ValueError('native_health_binary_owner')
            verify_manager(binary, expected)
            for entry in os.listdir(root):
                if entry != 'bin':
                    remove_entry(root, entry)
            for entry in os.listdir(binary):
                if entry != 'mise':
                    remove_entry(binary, entry)
            verify_manager(binary, expected)
        finally:
            os.close(binary)
    finally:
        os.close(root)


def cold_clear(path, boundary):
    root = open_root(path, boundary)
    try:
        for entry in os.listdir(root):
            remove_entry(root, entry)
    finally:
        os.close(root)


def cold_prepare_optional_manager(path, expected, boundary):
    # Planning bootstrap may populate Full using a manager in another domain.
    # Missing or unqualified Full managers are discarded, never executed.
    try:
        cold_prepare(path, expected, boundary)
    except (OSError, ValueError):
        cold_clear(path, boundary)

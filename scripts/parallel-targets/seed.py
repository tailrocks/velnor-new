"""Bounded, independent initialization of cold same-job Cargo check targets."""

import ctypes
import errno
import os
import pathlib
import re
import shutil
import stat
import sys
import time

SECONDS = 20
MAX_BYTES = 8 * 1024**3
MAX_ENTRIES = 64000
RESERVE = 2 * 1024**3
NOFOLLOW = os.O_NOFOLLOW
DIRECTORY = os.O_RDONLY | os.O_DIRECTORY | NOFOLLOW


class Budget(Exception):
    """Optional optimization cannot fit its fixed resource budget."""


def identity(value):
    return (value.st_dev, value.st_ino, value.st_mode, value.st_size,
            value.st_mtime_ns, value.st_ctime_ns, value.st_uid, value.st_gid, value.st_nlink)


def require(condition, message):
    if not condition:
        raise ValueError(message)


class Snapshot:
    def __init__(self):
        self.deadline = time.monotonic() + SECONDS
        self.entries = []
        self.bytes = 0

    def checkpoint(self):
        if time.monotonic() >= self.deadline:
            raise Budget("elapsed limit")

    def inventory(self, parent, prefix=()):
        self.checkpoint()
        require(len(prefix) <= 64, "source depth limit")
        for name in sorted(os.listdir(parent)):
            self.checkpoint()
            if prefix == ("debug",) and name == ".cargo-lock":
                continue
            value = os.stat(name, dir_fd=parent, follow_symlinks=False)
            require(stat.S_ISDIR(value.st_mode) or stat.S_ISREG(value.st_mode),
                    "source contains a symlink or special file")
            self.entries.append((prefix + (name,), value))
            if len(self.entries) > MAX_ENTRIES:
                raise Budget("entry limit")
            if stat.S_ISDIR(value.st_mode):
                child = os.open(name, DIRECTORY, dir_fd=parent)
                try:
                    require(identity(os.fstat(child)) == identity(value),
                            "source directory changed")
                    self.inventory(child, prefix + (name,))
                    require(identity(os.fstat(child)) == identity(value),
                            "source directory changed during inventory")
                finally:
                    os.close(child)
            else:
                self.bytes += value.st_size

    def copy_file(self, source, destination, expected):
        self.checkpoint()
        require(identity(os.fstat(source)) == identity(expected), "source file changed")
        count = 0
        while True:
            self.checkpoint()
            data = os.read(source, min(1024 * 1024, expected.st_size - count + 1))
            if not data:
                break
            count += len(data)
            require(count <= expected.st_size, "source grew beyond admitted byte budget")
            view = memoryview(data)
            while view:
                written = os.write(destination, view)
                require(written > 0, "copy made no progress")
                view = view[written:]
        require(count == expected.st_size and
                identity(os.fstat(source)) == identity(expected), "source changed during copy")
        current = os.fstat(destination)
        require(current.st_nlink == 1 and
                (current.st_dev, current.st_ino) != (expected.st_dev, expected.st_ino),
                "target copy is not independent")
        os.fchmod(destination, stat.S_IMODE(expected.st_mode))
        os.utime(destination, ns=(expected.st_atime_ns, expected.st_mtime_ns))


def open_relative(parent, parts):
    current = os.dup(parent)
    try:
        for part in parts:
            child = os.open(part, DIRECTORY, dir_fd=current)
            os.close(current)
            current = child
        return current
    except BaseException:
        os.close(current)
        raise


def copy_snapshot(snapshot, source, stage):
    for parts, expected in snapshot.entries:
        snapshot.checkpoint()
        parent = open_relative(source, parts[:-1])
        try:
            require(identity(os.stat(parts[-1], dir_fd=parent, follow_symlinks=False)) ==
                    identity(expected), "named source changed")
            output_parent = open_relative(stage, parts[:-1])
            try:
                if stat.S_ISDIR(expected.st_mode):
                    os.mkdir(parts[-1], mode=0o700, dir_fd=output_parent)
                else:
                    incoming = os.open(parts[-1], os.O_RDONLY | NOFOLLOW | os.O_NONBLOCK,
                                       dir_fd=parent)
                    try:
                        outgoing = os.open(parts[-1], os.O_WRONLY | os.O_CREAT | os.O_EXCL |
                                           NOFOLLOW, 0o600, dir_fd=output_parent)
                        try:
                            snapshot.copy_file(incoming, outgoing, expected)
                        finally:
                            os.close(outgoing)
                    finally:
                        os.close(incoming)
            finally:
                os.close(output_parent)
            require(identity(os.stat(parts[-1], dir_fd=parent, follow_symlinks=False)) ==
                    identity(expected), "named source changed during copy")
        finally:
            os.close(parent)
    for parts, expected in reversed(snapshot.entries):
        if stat.S_ISDIR(expected.st_mode):
            destination = open_relative(stage, parts)
            try:
                os.fchmod(destination, stat.S_IMODE(expected.st_mode))
                os.utime(destination, ns=(expected.st_atime_ns, expected.st_mtime_ns))
            finally:
                os.close(destination)


def validate_snapshot(snapshot, source):
    for parts, expected in snapshot.entries:
        snapshot.checkpoint()
        parent = open_relative(source, parts[:-1])
        try:
            require(identity(os.stat(parts[-1], dir_fd=parent, follow_symlinks=False)) ==
                    identity(expected), "baseline changed before admission")
        finally:
            os.close(parent)


def rename_exclusive(source, destination, parent):
    """Atomic admission must never replace a concurrently created lane."""
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "linux":
        function = getattr(libc, "renameat2", None)
        arguments = (parent, os.fsencode(source), parent, os.fsencode(destination), 1)
        types = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int,
                 ctypes.c_char_p, ctypes.c_uint)
    elif sys.platform == "darwin":
        function = getattr(libc, "renameatx_np", None)
        arguments = (parent, os.fsencode(source), parent, os.fsencode(destination), 4)
        types = (ctypes.c_int, ctypes.c_char_p, ctypes.c_int,
                 ctypes.c_char_p, ctypes.c_uint)
    else:
        raise Budget("atomic exclusive rename unsupported")
    if function is None:
        raise Budget("atomic exclusive rename unsupported")
    function.argtypes = types
    function.restype = ctypes.c_int
    if function(*arguments) != 0:
        code = ctypes.get_errno()
        if code in (errno.ENOSYS, errno.ENOTSUP):
            raise Budget("atomic exclusive rename unsupported")
        raise OSError(code, os.strerror(code))


def validate_arguments(target, lanes):
    parts = pathlib.PurePosixPath(target).parts
    require(parts and parts[-1] == "target" and not target.startswith("/") and
            all(re.fullmatch(r"[A-Za-z0-9_.-]+", part) and part not in (".", "..")
                for part in parts), "invalid scoped target")
    require(lanes and len(lanes) == len(set(lanes)) and
            set(lanes) <= {"velnor-clippy", "velnor-doc"}, "invalid target lanes")
    return parts


def seed(target, lanes):
    parts = validate_arguments(target, lanes)
    if sys.version_info < (3, 11):
        # Safe staging cleanup needs shutil.rmtree(dir_fd=...) (3.11+); this
        # is an optional optimization, so skip rather than crash mid-cleanup.
        print("Parallel targets: optional seed skipped: Python 3.11 or newer required")
        return
    workspace = pathlib.Path(os.environ["GITHUB_WORKSPACE"])
    require(workspace.is_absolute(), "workspace must be absolute")
    root = os.open(workspace, DIRECTORY)
    target_fd = None
    stages = []
    snapshot = Snapshot()
    try:
        try:
            target_fd = open_relative(root, parts)
        except FileNotFoundError:
            print("Parallel targets: no baseline; checks run cold")
            return
        held = os.fstat(target_fd)
        target_path = workspace.joinpath(*parts)
        require(identity(target_path.lstat()) == identity(held), "target path changed")
        cold = []
        for lane in lanes:
            try:
                value = os.stat(lane, dir_fd=target_fd, follow_symlinks=False)
            except FileNotFoundError:
                cold.append(lane)
                continue
            require(stat.S_ISDIR(value.st_mode), "existing lane is not a real directory")
            print("Parallel targets: preserve existing " + lane)
        if not cold:
            return
        try:
            debug = os.stat("debug", dir_fd=target_fd, follow_symlinks=False)
        except FileNotFoundError:
            print("Parallel targets: no debug baseline; checks run cold")
            return
        require(stat.S_ISDIR(debug.st_mode), "debug baseline is not a real directory")
        snapshot.entries.append((("debug",), debug))
        child = os.open("debug", DIRECTORY, dir_fd=target_fd)
        try:
            require(identity(os.fstat(child)) == identity(debug), "debug baseline changed")
            snapshot.inventory(child, ("debug",))
        finally:
            os.close(child)
        try:
            compiler = os.stat(".rustc_info.json", dir_fd=target_fd, follow_symlinks=False)
        except FileNotFoundError:
            compiler = None
        if compiler is not None:
            require(stat.S_ISREG(compiler.st_mode), "compiler metadata is not regular")
            snapshot.entries.append(((".rustc_info.json",), compiler))
            snapshot.bytes += compiler.st_size
        total = snapshot.bytes * len(cold)
        if total > MAX_BYTES or shutil.disk_usage(target_path).free < total + RESERVE:
            raise Budget("byte or free-disk limit")
        for lane in cold:
            snapshot.checkpoint()
            stage = ".velnor-seed-" + os.urandom(16).hex()
            os.mkdir(stage, mode=0o700, dir_fd=target_fd)
            stage_fd = os.open(stage, DIRECTORY, dir_fd=target_fd)
            held_stage = os.fstat(stage_fd)
            stages.append((stage, stage_fd, held_stage))
            require(identity(os.stat(stage, dir_fd=target_fd, follow_symlinks=False)) ==
                    identity(held_stage), "staging directory replaced before copy")
            copy_snapshot(snapshot, target_fd, stage_fd)
        validate_snapshot(snapshot, target_fd)
        for lane, (name, stage_fd, held_stage) in zip(cold, stages):
            snapshot.checkpoint()
            require((target_path.lstat().st_dev, target_path.lstat().st_ino) ==
                    (held.st_dev, held.st_ino), "target directory replaced")
            current = os.stat(name, dir_fd=target_fd, follow_symlinks=False)
            require((current.st_dev, current.st_ino) ==
                    (held_stage.st_dev, held_stage.st_ino), "staging directory replaced before admission")
            rename_exclusive(name, lane, target_fd)
            admitted = os.stat(lane, dir_fd=target_fd, follow_symlinks=False)
            require((admitted.st_dev, admitted.st_ino) ==
                    (held_stage.st_dev, held_stage.st_ino), "admitted lane identity changed")
            print(f"Parallel targets: seeded {lane}: {snapshot.bytes} bytes")
    except Budget as error:
        print("Parallel targets: optional seed skipped: " + str(error))
    except OSError as error:
        if error.errno not in (errno.ENOSPC, errno.EDQUOT):
            raise
        print("Parallel targets: optional seed skipped: storage capacity exhausted")
    finally:
        for name, stage_fd, held_stage in stages:
            try:
                os.close(stage_fd)
                current = os.stat(name, dir_fd=target_fd, follow_symlinks=False)
            except FileNotFoundError:
                continue
            require((current.st_dev, current.st_ino) ==
                    (held_stage.st_dev, held_stage.st_ino) and stat.S_ISDIR(current.st_mode),
                    "owned staging directory replaced; refusing cleanup")
            require(shutil.rmtree.avoids_symlink_attacks, "safe staging cleanup unsupported")
            shutil.rmtree(name, dir_fd=target_fd)
        if target_fd is not None:
            os.close(target_fd)
        os.close(root)


if __name__ == "__main__":
    seed(sys.argv[1], sys.argv[2:])

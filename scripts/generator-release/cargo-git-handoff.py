#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import re
import stat
import subprocess
import sys
import ctypes
from pathlib import Path

def move(src, name, dst, output):
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform == "darwin":
        rename = libc.renameatx_np
        flag = 4
    else:
        rename = libc.renameat2
        flag = 1
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    if rename(src, name.encode(), dst, output.encode(), flag) != 0:
        raise OSError(ctypes.get_errno(), "exclusive Cargo handoff rename")

def quarantine(source_home, quarantine_dir):
    if any(not os.path.isabs(path) or os.path.realpath(path) != path for path in (source_home, quarantine_dir)):
        raise ValueError("unsafe Cargo quarantine directory")
    src = os.open(source_home, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    dst = os.open(quarantine_dir, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    moved = []
    try:
        source_meta, quarantine_meta = os.fstat(src), os.fstat(dst)
        if source_meta.st_dev != quarantine_meta.st_dev or any(item.st_uid != os.getuid() or item.st_mode & 0o022 for item in (source_meta, quarantine_meta)):
            raise ValueError("unsafe Cargo quarantine ownership")
        for name in ("registry", "git", "config", "config.toml", "credentials", "credentials.toml"):
            try:
                original = os.stat(name, dir_fd=src, follow_symlinks=False)
            except FileNotFoundError:
                continue
            if original.st_uid != os.getuid():
                raise ValueError("unsafe Cargo cache entry ownership")
            move(src, name, dst, name)
            moved.append((name, original))
            after = os.stat(name, dir_fd=dst, follow_symlinks=False)
            if (original.st_dev, original.st_ino, original.st_mode) != (after.st_dev, after.st_ino, after.st_mode):
                raise ValueError("Cargo cache entry identity changed")
    except Exception:
        for name, original in reversed(moved):
            try:
                current = os.stat(name, dir_fd=dst, follow_symlinks=False)
                if (current.st_dev, current.st_ino, current.st_mode) == (original.st_dev, original.st_ino, original.st_mode):
                    move(dst, name, src, name)
            except OSError:
                pass
        raise
    finally:
        os.close(src)
        os.close(dst)

def install(source_home, staging, quarantine):
    paths = [source_home, staging, quarantine]
    if any(not os.path.isabs(path) or os.path.realpath(path) != path for path in paths):
        raise ValueError("unsafe Git handoff directory")
    fds = [os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW) for path in paths]
    try:
        metadata = [os.fstat(fd) for fd in fds]
        if any(not stat.S_ISDIR(item.st_mode) or item.st_uid != os.getuid() or item.st_mode & 0o022 for item in metadata):
            raise ValueError("unsafe Git handoff ownership")
        if len({item.st_dev for item in metadata}) != 1:
            raise ValueError("Git handoff crosses filesystems")
        try:
            original = os.stat("git", dir_fd=fds[0], follow_symlinks=False)
        except FileNotFoundError:
            return
        if original.st_uid != os.getuid():
            raise ValueError("unsafe Git source ownership")
        move(fds[0], "git", fds[2], "original")
        try:
            moved = os.stat("original", dir_fd=fds[2], follow_symlinks=False)
            if (moved.st_dev, moved.st_ino, moved.st_mode) != (original.st_dev, original.st_ino, original.st_mode):
                raise ValueError("Git handoff identity changed")
            try:
                replacement = os.stat("git", dir_fd=fds[1], follow_symlinks=False)
            except FileNotFoundError:
                return
            if not stat.S_ISDIR(replacement.st_mode) or replacement.st_uid != os.getuid():
                raise ValueError("unsafe Git replacement")
            move(fds[1], "git", fds[0], "git")
            published = os.stat("git", dir_fd=fds[0], follow_symlinks=False)
            if (published.st_dev, published.st_ino, published.st_mode) != (replacement.st_dev, replacement.st_ino, replacement.st_mode):
                raise ValueError("Git replacement identity changed")
        except Exception:
            try:
                os.stat("git", dir_fd=fds[0], follow_symlinks=False)
            except FileNotFoundError:
                current = os.stat("original", dir_fd=fds[2], follow_symlinks=False)
                if (current.st_dev, current.st_ino, current.st_mode) == (original.st_dev, original.st_ino, original.st_mode):
                    move(fds[2], "original", fds[0], "git")
            raise
    finally:
        for fd in fds:
            os.close(fd)

if len(sys.argv) == 4 and sys.argv[1] == "quarantine":
    try:
        quarantine(*sys.argv[2:])
    except (OSError, ValueError):
        print("unsafe Cargo source quarantine", file=sys.stderr)
        sys.exit(2)
    sys.exit(0)

if len(sys.argv) == 5 and sys.argv[1] == "install":
    try:
        install(*sys.argv[2:])
    except (OSError, ValueError):
        print("unsafe Git source quarantine", file=sys.stderr)
        sys.exit(2)
    sys.exit(0)

inspect_only = len(sys.argv) == 6 and sys.argv[1] == "inspect"
source, destination, entry_count, byte_count = sys.argv[2:] if inspect_only else sys.argv[1:]
entries = int(entry_count)
size_total = int(byte_count)
flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW

def fail():
    raise ValueError("unsafe Git source cache")

def open_dir(parent, name):
    fd = os.open(name, flags | os.O_DIRECTORY, dir_fd=parent)
    if not stat.S_ISDIR(os.fstat(fd).st_mode):
        fail()
    return fd

def names(fd):
    global entries
    result = os.listdir(fd)
    entries += len(result)
    if entries > 10000:
        fail()
    return result

def copy_file(parent, name, dest):
    global size_total
    expected = os.stat(name, dir_fd=parent, follow_symlinks=False)
    if not stat.S_ISREG(expected.st_mode) or expected.st_nlink != 1:
        fail()
    fd = os.open(name, flags | os.O_NONBLOCK, dir_fd=parent)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or (expected.st_dev, expected.st_ino, expected.st_mode) != (before.st_dev, before.st_ino, before.st_mode):
            fail()
        size_total += before.st_size
        if size_total > 536870912:
            fail()
        if dest is not None:
            with open(dest, "xb") as out:
                remaining = before.st_size
                while remaining:
                    chunk = os.read(fd, min(1048576, remaining))
                    if not chunk:
                        fail()
                    out.write(chunk)
                    remaining -= len(chunk)
        after = os.fstat(fd)
        if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            fail()
    finally:
        os.close(fd)

def command(args, home):
    subprocess.run(args, check=True, timeout=120, stdin=subprocess.DEVNULL,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                   env={"PATH": "/usr/bin:/bin", "HOME": str(home), "LC_ALL": "C", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_TERMINAL_PROMPT": "0", "GIT_NO_REPLACE_OBJECTS": "1"})

def copy_objects(db, target):
    objects = open_dir(db, "objects")
    try:
        for item in names(objects):
            if item == "info":
                info = open_dir(objects, item)
                try:
                    if any(name in ("alternates", "http-alternates") for name in names(info)):
                        fail()
                finally:
                    os.close(info)
            elif item == "pack":
                pack = open_dir(objects, item)
                try:
                    files = names(pack)
                    selected = set()
                    for name in files:
                        match = re.fullmatch(r"pack-([0-9a-f]{40})\.(pack|idx)", name)
                        if match:
                            selected.add((match.group(1), match.group(2)))
                        elif name.endswith(".promisor"):
                            fail()
                        elif not re.fullmatch(r"(?:pack-[0-9a-f]{40}\.(?:rev|bitmap|keep)|multi-pack-index)", name):
                            fail()
                    if any((digest, "pack") not in selected or (digest, "idx") not in selected for digest, _ in selected):
                        fail()
                    for digest, suffix in sorted(selected):
                        name = f"pack-{digest}.{suffix}"
                        copy_file(pack, name, None if target is None else target / "objects" / "pack" / name)
                finally:
                    os.close(pack)
            elif re.fullmatch(r"[0-9a-f]{2}", item):
                loose = open_dir(objects, item)
                try:
                    if target is not None:
                        (target / "objects" / item).mkdir(mode=0o700)
                    for name in names(loose):
                        if not re.fullmatch(r"[0-9a-f]{38}", name):
                            fail()
                        copy_file(loose, name, None if target is None else target / "objects" / item / name)
                finally:
                    os.close(loose)
            else:
                fail()
    finally:
        os.close(objects)

try:
    src = os.open(source, flags | os.O_DIRECTORY)
    try:
        try:
            git = open_dir(src, "git")
        except FileNotFoundError:
            sys.exit(0)
        try:
            try:
                databases = open_dir(git, "db")
            except FileNotFoundError:
                sys.exit(0)
            try:
                root = Path(destination)
                git_root = root / "git"
                db_root = git_root / "db"
                template = git_root / "empty-template"
                if not inspect_only:
                    git_root.mkdir(mode=0o700)
                    db_root.mkdir(mode=0o700)
                    template.mkdir(mode=0o700)
                for name in names(databases):
                    if not re.fullmatch(r"[^/]+-[0-9a-f]{16}", name) or name in (".", ".."):
                        fail()
                    db = open_dir(databases, name)
                    try:
                        target = None if inspect_only else db_root / name
                        if target is not None:
                            command(["/usr/bin/git", "init", "--bare", "--object-format=sha1", "--template=" + str(template), str(target)], root)
                        copy_objects(db, target)
                    finally:
                        os.close(db)
                if not inspect_only:
                    template.rmdir()
            finally:
                os.close(databases)
        finally:
            os.close(git)
    finally:
        os.close(src)
except (OSError, ValueError, subprocess.SubprocessError):
    print("unsafe or incomplete Git source handoff", file=sys.stderr)
    sys.exit(1)

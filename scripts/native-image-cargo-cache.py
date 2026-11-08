#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
import re
import stat
import sys
import time
import tomllib

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
CACHE = "index.crates.io-1949cf8c6b5b557f"
MAX_LOCK = 8 * 1024 * 1024
MAX_FILES = 4096
MAX_ARCHIVE = 128 * 1024 * 1024
MAX_TOTAL = 1024 * 1024 * 1024
MAX_SECONDS = 30
FLAGS = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW
DIR_FLAGS = FLAGS | os.O_DIRECTORY


def directory(path):
    if not path.startswith("/") or any(part in ("", ".", "..") for part in path.split("/")[1:]):
        raise ValueError("unsafe absolute path")
    current = os.open("/", DIR_FLAGS)
    try:
        for part in path.split("/")[1:]:
            child = os.open(part, DIR_FLAGS, dir_fd=current)
            os.close(current)
            current = child
        return current
    except BaseException:
        os.close(current)
        raise


def child_directory(parent, name, create=False):
    if create:
        try:
            os.mkdir(name, 0o700, dir_fd=parent)
        except FileExistsError:
            pass
    return os.open(name, DIR_FLAGS, dir_fd=parent)


def stable(info):
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def lock_entries(workspace, mode):
    parts = ["Cargo.lock"] if mode == "workspace" else [
        "crates", "velnor-actions-cli", "tests", "fixtures", "native-image-qualification", "Cargo.lock"
    ]
    current = directory(workspace)
    try:
        for part in parts[:-1]:
            child = child_directory(current, part)
            os.close(current)
            current = child
        fd = os.open(parts[-1], FLAGS | os.O_NONBLOCK, dir_fd=current)
        try:
            before = os.fstat(fd)
            if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_size > MAX_LOCK:
                raise ValueError("invalid lock")
            with os.fdopen(fd, "rb", closefd=False) as file:
                data = file.read(MAX_LOCK + 1)
            if len(data) != before.st_size or stable(os.fstat(fd)) != stable(before):
                raise ValueError("lock changed")
        finally:
            os.close(fd)
    finally:
        os.close(current)
    parsed = tomllib.loads(data.decode("utf-8"))
    packages = parsed.get("package")
    if not isinstance(packages, list) or len(packages) > MAX_FILES:
        raise ValueError("invalid package list")
    entries = {}
    identities = set()
    for package in packages:
        if not isinstance(package, dict):
            raise ValueError("invalid package")
        source = package.get("source")
        name, version = package.get("name"), package.get("version")
        if (not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", name)
                or not isinstance(version, str) or not re.fullmatch(r"[0-9A-Za-z][0-9A-Za-z.+-]*", version)
                or (source is not None and not isinstance(source, str))):
            raise ValueError("invalid package identity")
        identity = (name, version, source)
        if identity in identities:
            raise ValueError("duplicate package identity")
        identities.add(identity)
        if source != REGISTRY:
            continue
        checksum = package.get("checksum")
        if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise ValueError("invalid registry checksum")
        filename = f"{name}-{version}.crate"
        if filename in entries:
            raise ValueError("ambiguous registry package")
        entries[filename] = checksum
    return entries


def open_cache(home, create=False):
    current = directory(home)
    try:
        for name in ("registry", "cache", CACHE):
            child = child_directory(current, name, create)
            os.close(current)
            current = child
        return current
    except BaseException:
        os.close(current)
        raise


def transfer(source, target, name, expected, remaining, deadline):
    try:
        source_fd = os.open(name, FLAGS | os.O_NONBLOCK, dir_fd=source)
    except OSError:
        return 0
    staged = "." + name + ".part"
    out_fd = None
    stage_identity = None
    try:
        before = os.fstat(source_fd)
        if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1
                or before.st_size <= 0 or before.st_size > min(MAX_ARCHIVE, remaining)):
            return 0
        out_fd = os.open(staged, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW,
                         0o600, dir_fd=target)
        info = os.fstat(out_fd)
        stage_identity = (info.st_dev, info.st_ino)
        digest = hashlib.sha256()
        count = 0
        while True:
            if time.monotonic() >= deadline:
                return 0
            data = os.read(source_fd, 1024 * 1024)
            if not data:
                break
            count += len(data)
            if count > before.st_size:
                return 0
            digest.update(data)
            view = memoryview(data)
            while view:
                written = os.write(out_fd, view)
                if written <= 0:
                    return 0
                view = view[written:]
        after = os.fstat(source_fd)
        current = os.stat(name, dir_fd=source, follow_symlinks=False)
        if (count != before.st_size or digest.hexdigest() != expected or stable(after) != stable(before)
                or stable(current) != stable(before)):
            return 0
        os.close(out_fd)
        out_fd = None
        verify_fd = os.open(staged, FLAGS | os.O_NONBLOCK, dir_fd=target)
        try:
            stage_info = os.fstat(verify_fd)
            if (not stat.S_ISREG(stage_info.st_mode) or stage_info.st_nlink != 1
                    or stage_info.st_size != count):
                return 0
            check = hashlib.sha256()
            while True:
                if time.monotonic() >= deadline:
                    return 0
                data = os.read(verify_fd, 1024 * 1024)
                if not data:
                    break
                check.update(data)
            if (check.hexdigest() != expected or stable(os.fstat(verify_fd)) != stable(stage_info)
                    or stable(os.stat(staged, dir_fd=target, follow_symlinks=False)) != stable(stage_info)):
                return 0
        finally:
            os.close(verify_fd)
        os.link(staged, name, src_dir_fd=target, dst_dir_fd=target, follow_symlinks=False)
        return count
    except OSError:
        return 0
    finally:
        if out_fd is not None:
            os.close(out_fd)
        if stage_identity is not None:
            try:
                info = os.stat(staged, dir_fd=target, follow_symlinks=False)
            except FileNotFoundError:
                pass
            else:
                if (info.st_dev, info.st_ino) != stage_identity:
                    raise ValueError("stage replaced")
                os.unlink(staged, dir_fd=target)
        os.close(source_fd)


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in ("workspace", "fixture"):
        raise ValueError("invalid mode")
    workspace = os.environ["GITHUB_WORKSPACE"]
    temp = os.environ["RUNNER_TEMP"]
    mode = sys.argv[1]
    home = (temp + "/velnor-qualification-runtime/cargo" if mode == "workspace"
            else temp + "/velnor-native-image-qualification/cargo")
    if os.environ.get("CARGO_HOME") != home:
        raise ValueError("invalid private Cargo home")
    deadline = time.monotonic() + MAX_SECONDS
    try:
        entries = lock_entries(workspace, mode)
        source = open_cache(temp + "/velnor/cargo")
    except (KeyError, OSError, UnicodeError, ValueError, tomllib.TOMLDecodeError):
        print(f"velnor: native archive reuse mode={mode} copied=0 bytes=0")
        return
    try:
        target = open_cache(home, create=True)
        try:
            count = total = 0
            for name, expected in sorted(entries.items()):
                if total >= MAX_TOTAL or time.monotonic() >= deadline:
                    break
                size = transfer(source, target, name, expected, MAX_TOTAL - total, deadline)
                if size:
                    count += 1
                    total += size
            print(f"velnor: native archive reuse mode={mode} copied={count} bytes={total}")
        finally:
            os.close(target)
    finally:
        os.close(source)


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, ValueError):
        print("velnor: native archive reuse failed", file=sys.stderr)
        sys.exit(1)

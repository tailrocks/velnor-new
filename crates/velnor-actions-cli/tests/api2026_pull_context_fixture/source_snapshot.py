# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import os
import pathlib
import stat
import subprocess

MAX_TOTAL = 512 * 1024 * 1024
MAX_ENTRIES = 100000
MAX_LINK = 4096
MAX_GIT_OUTPUT = 64 * 1024 * 1024


def git_output(args, env):
    with subprocess.Popen(args, env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL) as child:
        data = child.stdout.read(MAX_GIT_OUTPUT + 1)
        if len(data) > MAX_GIT_OUTPUT:
            child.kill()
        status = child.wait()
    if len(data) > MAX_GIT_OUTPUT:
        raise RuntimeError("fixture Git input exceeds its bound")
    if status:
        raise RuntimeError("fixture Git input enumeration failed")
    return data


def git_inputs(root):
    env = {"PATH": "/usr/bin:/bin", "HOME": str(pathlib.Path.home()), "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_OPTIONAL_LOCKS": "0"}
    base = ["/usr/bin/git", "-C", str(root), "ls-files", "-z"]
    tracked = git_output(base + ["--cached", "--stage"], env)
    if git_output(base + ["--others", "--exclude-standard"], env):
        raise RuntimeError("untracked workspace files prevent a complete fixture snapshot")
    entries = list(filter(None, tracked.split(b"\0")))
    if len(entries) > MAX_ENTRIES:
        raise RuntimeError("too many tracked fixture inputs")
    return sorted(entries, key=lambda item: item.split(b"\t", 1)[1])


def read_link(root, name):
    parts = name.split(b"/")
    current = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=current)
            os.close(current)
            current = child
        before = os.stat(parts[-1], dir_fd=current, follow_symlinks=False)
        if not stat.S_ISLNK(before.st_mode):
            raise RuntimeError("tracked link changed type")
        target = os.readlink(parts[-1], dir_fd=current)
        after = os.stat(parts[-1], dir_fd=current, follow_symlinks=False)
        if len(target) > MAX_LINK or (before.st_ino, before.st_mtime_ns, before.st_mode) != (after.st_ino, after.st_mtime_ns, after.st_mode):
            raise RuntimeError("tracked link changed while reading")
        return target
    finally:
        os.close(current)


def check_gitlink(root, name):
    current = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        for part in name.split(b"/"):
            try:
                child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=current)
            except FileNotFoundError:
                return
            os.close(current)
            current = child
        if os.listdir(current):
            raise RuntimeError("initialized submodule prevents complete fixture snapshot")
    finally:
        os.close(current)


def directories(entries):
    result = set()
    for name, record in entries.items():
        if record[1] == b"missing":
            continue
        parts = name.split(b"/")
        result.update(b"/".join(parts[:i]) for i in range(1, len(parts)))
        if record[1] == b"gitlink":
            result.add(name)
    return result


def check_links(entries, dirs):
    directory_links = []
    for name, record in entries.items():
        if record[1] != b"link":
            continue
        pending = name.split(b"/")
        resolved = []
        hops = 0
        while pending:
            part = pending.pop(0)
            if part in (b"", b"."):
                continue
            if part == b"..":
                if not resolved:
                    raise RuntimeError("fixture link escapes tracked source")
                resolved.pop()
                continue
            resolved.append(part)
            current = b"/".join(resolved)
            item = entries.get(current)
            if item and item[1] == b"link":
                target = item[4]
                if target.startswith(b"/"):
                    raise RuntimeError("absolute fixture link is not allowed")
                hops += 1
                if hops > 32:
                    raise RuntimeError("cyclic fixture link")
                resolved.pop()
                pending = target.split(b"/") + pending
        final = b"/".join(resolved)
        item = entries.get(final)
        if final and final not in dirs and (not item or item[1] != b"file"):
            raise RuntimeError("fixture link does not resolve to tracked content")
        if not final or final in dirs:
            directory_links.append((name.rpartition(b"/")[0], final))
    nodes = dirs | {b""}
    edges = {node: set() for node in nodes}
    for directory in dirs:
        edges[directory.rpartition(b"/")[0]].add(directory)
    for parent, target in directory_links:
        edges[parent].add(target)
    incoming = {node: 0 for node in nodes}
    for targets in edges.values():
        for target in targets:
            incoming[target] += 1
    pending = [node for node, count in incoming.items() if count == 0]
    visited = 0
    while pending:
        node = pending.pop()
        visited += 1
        for target in edges[node]:
            incoming[target] -= 1
            if incoming[target] == 0:
                pending.append(target)
    if visited != len(nodes):
        raise RuntimeError("cyclic fixture directory links")


def collect(root, destination, read_tracked):
    result = {}
    total = 0
    for entry in git_inputs(root):
        metadata, name = entry.split(b"\t", 1)
        mode, oid, stage = metadata.split(b" ")
        if stage != b"0" or any(part in (b"", b".", b"..") for part in name.split(b"/")):
            raise RuntimeError("invalid tracked fixture input")
        if mode == b"160000":
            check_gitlink(root, name)
            result[name] = (mode, b"gitlink", 0, 0, oid)
            continue
        if mode == b"120000":
            try:
                target = read_link(root, name)
            except FileNotFoundError:
                result[name] = (mode, b"missing", 0, 0, b"")
            else:
                result[name] = (mode, b"link", 0, len(target), target)
            continue
        if mode not in (b"100644", b"100755"):
            raise RuntimeError("unsupported tracked fixture input")
        try:
            data, file_mode = read_tracked(root, name)
        except FileNotFoundError:
            result[name] = (mode, b"missing", 0, 0, b"")
            continue
        total += len(data)
        if total > MAX_TOTAL:
            raise RuntimeError("fixture workspace exceeds snapshot bound")
        result[name] = (mode, b"file", file_mode, len(data), hashlib.sha256(data).digest())
        if destination is not None:
            output = destination / os.fsdecode(name)
            output.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            fd = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, file_mode)
            try:
                with os.fdopen(fd, "wb", closefd=False) as stream:
                    stream.write(data)
                os.fchmod(fd, file_mode)
            finally:
                os.close(fd)
    dirs = directories(result)
    check_links(result, dirs)
    if destination is not None:
        for name in sorted(dirs):
            (destination / os.fsdecode(name)).mkdir(mode=0o700, parents=True, exist_ok=True)
        for name, record in result.items():
            if record[1] == b"link":
                os.symlink(record[4], os.fsencode(destination) + b"/" + name)
    return result


def digest(entries):
    result = hashlib.sha256()
    for name, record in sorted(entries.items()):
        for item in (name, record[0], record[1], str(record[2]).encode(), str(record[3]).encode(), record[4]):
            result.update(len(item).to_bytes(8, "big")); result.update(item)
    return result.digest()


def verify_snapshot(root, entries, read_tracked):
    dirs = directories(entries)
    expected = {name for name, record in entries.items() if record[1] in (b"file", b"link")} | dirs
    found = set()
    for parent, subdirs, files in os.walk(root, followlinks=False, onerror=lambda error: (_ for _ in ()).throw(error)):
        for name in subdirs + files:
            path = pathlib.Path(parent) / name
            relative = os.fsencode(path.relative_to(root))
            found.add(relative)
            record = entries.get(relative)
            if path.is_symlink():
                if not record or record[1] != b"link" or read_link(root, relative) != record[4]:
                    raise RuntimeError("fixture snapshot link changed")
            elif path.is_dir():
                if relative not in dirs:
                    raise RuntimeError("fixture snapshot directory changed")
            elif record and record[1] == b"file":
                data, mode = read_tracked(root, relative)
                if (mode, len(data), hashlib.sha256(data).digest()) != record[2:]:
                    raise RuntimeError("fixture snapshot file changed")
            else:
                raise RuntimeError("fixture snapshot member changed")
    if found != expected:
        raise RuntimeError("fixture snapshot membership changed")

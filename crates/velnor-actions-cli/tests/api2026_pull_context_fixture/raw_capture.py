# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
import pathlib
import re
import select
import stat
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import driver
import source_snapshot as snapshot

MAX_FILE = 128 * 1024 * 1024


def git_env():
    return {"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_NO_REPLACE_OBJECTS": "1",
            "GIT_NO_LAZY_FETCH": "1", "GIT_OPTIONAL_LOCKS": "0"}


def git(root, *args, data=None):
    if data is not None and len(data) > snapshot.MAX_GIT_OUTPUT:
        raise RuntimeError("raw fixture Git input exceeds bound")
    with tempfile.TemporaryFile() as input_file:
        if data is not None:
            input_file.write(data)
            input_file.seek(0)
        process = subprocess.Popen(["/usr/bin/git", "-c", "core.fsmonitor=false", "-c", "core.ignorecase=false",
                                    "-c", "protocol.allow=never", "-C", str(root), *args], stdin=input_file,
                                   stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=git_env())
        try:
            deadline = time.monotonic() + 30
            output = bytearray()
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([process.stdout], [], [], remaining)[0]:
                    raise RuntimeError("raw fixture Git command timed out")
                chunk = os.read(process.stdout.fileno(), min(65536, snapshot.MAX_GIT_OUTPUT + 1 - len(output)))
                if not chunk:
                    break
                output.extend(chunk)
                if len(output) > snapshot.MAX_GIT_OUTPUT:
                    raise RuntimeError("raw fixture Git output exceeds bound")
            if process.wait(timeout=max(0.001, deadline - time.monotonic())):
                raise RuntimeError("raw fixture Git command failed")
            return bytes(output)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            if process.stdout:
                process.stdout.close()


def inventory(root):
    raw = git(root, "ls-files", "--stage", "-z")
    flags = snapshot.nul_records(git(root, "ls-files", "-t", "-z"))
    if git(root, "ls-files", "--others", "--exclude-standard", "-z"):
        raise RuntimeError("untracked fixture input")
    states = {}
    for flag in flags:
        if len(flag) < 3 or flag[1:2] != b" " or flag[:1] not in (b"H", b"S") or flag[2:] in states:
            raise RuntimeError("invalid fixture flags")
        states[flag[2:]] = flag[:1]
    records = snapshot.nul_records(raw)
    if not records or len(records) > snapshot.MAX_ENTRIES:
        raise RuntimeError("invalid fixture inventory")
    parsed = []
    for record in records:
        meta, sep, name = record.partition(b"\t")
        fields = meta.split(b" ")
        if (not sep or len(fields) != 3 or fields[0] not in (b"100644", b"100755", b"120000", b"160000")
                or fields[2] != b"0" or not re.fullmatch(b"[0-9a-f]{40}", fields[1])
                or not name or any(part in (b"", b".", b"..", b".git") for part in name.split(b"/"))):
            raise RuntimeError("invalid fixture index entry")
        parsed.append((name, fields[0], fields[1]))
    names = [name for name, _, _ in parsed]
    if len(set(names)) != len(names) or set(names) != set(states):
        raise RuntimeError("fixture index changed")
    return raw, states, parsed


def blobs(root, oids):
    command = ["/usr/bin/git", "-c", "core.fsmonitor=false", "-c", "protocol.allow=never",
               "-C", str(root), "cat-file", "--batch"]
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL, env=git_env())
    deadline = time.monotonic() + 120
    def read_exact(size):
        output = bytearray()
        while len(output) < size:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([process.stdout], [], [], remaining)[0]:
                raise RuntimeError("fixture blob reader timed out")
            chunk = os.read(process.stdout.fileno(), min(65536, size - len(output)))
            if not chunk:
                raise RuntimeError("truncated indexed fixture blob")
            output.extend(chunk)
        return bytes(output)
    try:
        for oid in oids:
            process.stdin.write(oid + b"\n"); process.stdin.flush()
            header = bytearray()
            while len(header) < 256 and not header.endswith(b"\n"):
                header.extend(read_exact(1))
            if not header.endswith(b"\n"):
                raise RuntimeError("invalid indexed fixture blob header")
            parts = header.rstrip(b"\n").split(b" ")
            if len(parts) != 3 or parts[0] != oid or parts[1] != b"blob" or not parts[2].isdigit():
                raise RuntimeError("indexed fixture blob missing")
            size = int(parts[2])
            if size > MAX_FILE:
                raise RuntimeError("indexed fixture blob too large")
            data = read_exact(size + 1)
            if len(data) != size + 1 or data[-1:] != b"\n":
                raise RuntimeError("truncated indexed fixture blob")
            yield oid, data[:-1]
        process.stdin.close()
        if process.wait(timeout=max(0.001, deadline - time.monotonic())):
            raise RuntimeError("fixture blob reader failed")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        if process.stdout:
            process.stdout.close()


def capture(root, destination, manifest):
    root, destination = pathlib.Path(root), pathlib.Path(destination)
    if not root.is_absolute() or not destination.is_absolute() or any(destination.iterdir()):
        raise RuntimeError("invalid raw fixture destination")
    raw, flags, entries = inventory(root)
    result, values, hydrated = {}, {}, []
    total = 0
    for name, mode, oid in entries:
        if mode == b"160000":
            snapshot.check_gitlink(root, name)
            missing = flags[name] == b"S" and snapshot.excluded_absent(root, name)
            values[name] = (mode, oid, flags[name], missing, b"", 0)
            continue
        try:
            if mode == b"120000":
                data = snapshot.read_link(root, name); file_mode = 0
            else:
                data, file_mode = driver.read_tracked(root, name)
            missing = False
        except FileNotFoundError:
            if flags[name] != b"S":
                raise RuntimeError("unreviewed missing fixture source")
            missing = True; data = None
            file_mode = 0 if mode == b"120000" else (0o755 if mode == b"100755" else 0o644)
        if data is not None:
            total += len(data)
            if total > snapshot.MAX_TOTAL:
                raise RuntimeError("fixture capture exceeds bound")
        values[name] = (mode, oid, flags[name], missing, data, file_mode)
        if missing:
            hydrated.append((name, oid))
    loaded = iter(blobs(root, [oid for _, oid in hydrated]))
    for name, oid in hydrated:
        actual, data = next(loaded)
        if actual != oid:
            raise RuntimeError("fixture blob identity changed")
        total += len(data)
        if total > snapshot.MAX_TOTAL:
            raise RuntimeError("fixture capture exceeds bound")
        mode, _, flag, missing, _, file_mode = values[name]
        values[name] = (mode, oid, flag, missing, data, file_mode)
    try:
        next(loaded)
        raise RuntimeError("extra fixture blob")
    except StopIteration:
        pass
    for name, (mode, oid, flag, missing, data, file_mode) in values.items():
        kind = b"link" if mode == b"120000" else b"gitlink" if mode == b"160000" else b"file"
        digest = data if kind == b"link" else oid if kind == b"gitlink" else hashlib.sha256(data).digest()
        result[name] = (mode, kind, file_mode, len(data), digest)
    dirs = snapshot.directories(result)
    snapshot.check_links(result, dirs)
    for name in sorted(dirs):
        (destination / os.fsdecode(name)).mkdir(mode=0o700, parents=True, exist_ok=True)
    for name, (mode, oid, flag, missing, data, file_mode) in values.items():
        output = os.fsencode(destination) + b"/" + name
        if mode == b"160000":
            continue
        if mode == b"120000":
            os.symlink(data, output)
        else:
            fd = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, file_mode)
            try:
                with os.fdopen(fd, "wb", closefd=False) as stream:
                    stream.write(data)
                os.fchmod(fd, file_mode)
            finally:
                os.close(fd)
    if inventory(root) != (raw, flags, entries):
        raise RuntimeError("fixture index changed during capture")
    snapshot.verify_snapshot(destination, result, driver.read_tracked)
    origin = git(root, "config", "--get", "remote.origin.url").strip()
    if (not origin or len(origin) > 2048
            or not re.fullmatch(rb"(?:https://[A-Za-z0-9.-]+/[A-Za-z0-9._/-]+|git@[A-Za-z0-9.-]+:[A-Za-z0-9._/-]+)", origin)):
        raise RuntimeError("fixture origin unavailable")
    git(destination, "init", "--quiet")
    git(destination, "remote", "add", "origin", origin.decode("ascii"))
    git(destination, "update-index", "--info-only", "-z", "--index-info", data=raw)
    if git(destination, "ls-files", "--stage", "-z") != raw:
        raise RuntimeError("private fixture index mismatch")
    payload = {"schema": 1, "index_sha256": hashlib.sha256(raw).hexdigest(), "entries": [
        {"path_hex": name.hex(), "mode": mode.decode(), "oid": oid.decode(), "skip": flag == b"S",
         "hydrated": missing, "physical_mode": file_mode, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        for name, (mode, oid, flag, missing, data, file_mode) in sorted(values.items())]}
    fd = os.open(manifest, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "w", encoding="ascii") as stream:
        json.dump(payload, stream, separators=(",", ":"), sort_keys=True)
    return payload


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit("invalid raw fixture capture")
    capture(*sys.argv[1:])


def project(validation, destination, manifest, patterns):
    validation, destination = pathlib.Path(validation), pathlib.Path(destination)
    if (not validation.is_absolute() or not destination.is_absolute() or any(destination.iterdir())
            or manifest.get("schema") != 1 or not isinstance(patterns, list)
            or len(set(patterns)) != len(patterns)
            or any(not isinstance(p, str) or not re.fullmatch(r"(?:[A-Za-z0-9._-]+|\*)(?:/(?:[A-Za-z0-9._-]+|\*))*/\*\*", p)
                   or any(set(segment) == {"."} for segment in p[:-3].split("/")) for p in patterns)):
        raise RuntimeError("invalid fixture projection")
    index = git(validation, "ls-files", "--stage", "-z")
    if hashlib.sha256(index).hexdigest() != manifest.get("index_sha256"):
        raise RuntimeError("private fixture index changed")
    matched = snapshot.nul_records(git(validation, "ls-files", "--cached", "-z", "--",
                                       *[":(glob)" + p for p in patterns])) if patterns else []
    matches = set(matched)
    if len(matches) != len(matched):
        raise RuntimeError("duplicate fixture exclusion inventory")
    entries, result, values = manifest.get("entries"), {}, {}
    if not isinstance(entries, list) or not entries or len(entries) > snapshot.MAX_ENTRIES:
        raise RuntimeError("invalid fixture manifest")
    for entry in entries:
        if set(entry) != {"path_hex", "mode", "oid", "skip", "hydrated", "physical_mode", "size", "sha256"}:
            raise RuntimeError("invalid fixture manifest entry")
        name = bytes.fromhex(entry["path_hex"])
        if (not name or name.hex() != entry["path_hex"] or name in result
                or any(part in (b"", b".", b"..", b".git") for part in name.split(b"/"))
                or type(entry["skip"]) is not bool or type(entry["hydrated"]) is not bool):
            raise RuntimeError("invalid fixture manifest name")
        if entry["skip"] and name not in matches:
            raise RuntimeError("fixture skip-worktree is outside reviewed exclusions")
        if entry["hydrated"]:
            if not entry["skip"] or name not in matches:
                raise RuntimeError("unreviewed fixture omission")
            continue
        mode = entry["mode"].encode("ascii")
        if mode == b"160000":
            snapshot.check_gitlink(validation, name); data = b""; physical_mode = 0; kind = b"gitlink"
        elif mode == b"120000":
            data = snapshot.read_link(validation, name); physical_mode = 0; kind = b"link"
        elif mode in (b"100644", b"100755"):
            data, physical_mode = driver.read_tracked(validation, name); kind = b"file"
        else:
            raise RuntimeError("unsupported fixture projection mode")
        if (len(data) != entry["size"] or physical_mode != entry["physical_mode"]
                or hashlib.sha256(data).hexdigest() != entry["sha256"]):
            raise RuntimeError("fixture capture changed")
        values[name] = (mode, data, physical_mode)
        digest = data if kind == b"link" else entry["oid"].encode("ascii") if kind == b"gitlink" else hashlib.sha256(data).digest()
        result[name] = (mode, kind, physical_mode, len(data), digest)
    dirs = snapshot.directories(result)
    snapshot.check_links(result, dirs)
    for name in sorted(dirs):
        (destination / os.fsdecode(name)).mkdir(mode=0o700, parents=True, exist_ok=True)
    for name, (mode, data, physical_mode) in values.items():
        path = os.fsencode(destination) + b"/" + name
        if mode == b"160000":
            continue
        if mode == b"120000":
            os.symlink(data, path)
        else:
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, physical_mode)
            try:
                with os.fdopen(fd, "wb", closefd=False) as stream:
                    stream.write(data)
                os.fchmod(fd, physical_mode)
            finally:
                os.close(fd)
    snapshot.verify_snapshot(destination, result, driver.read_tracked)
    return result

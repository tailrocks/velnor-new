#!/usr/bin/python3
"""Exercise the reviewed Rustup stage boundary and exact-size copy path."""

import hashlib
import json
import os
import pathlib
import resource
import stat
import sys
import tempfile
import types

TASK = "/root/.velnor-generator-preview-20261005"
STAGE = TASK + "/rootfs-rustup-delta-v1.py"
ASSET = TASK + "/assets/rustup-init-1.28.2-x86_64-unknown-linux-gnu"
STAGE_SHA256 = "29b080a5fb5a0fd72164ac11ab094feaf303bac4efd6fee29e087cebd47dad2f"
ASSET_SHA256 = "20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c"
ASSET_SIZE = 20889000


def fail(message):
    raise SystemExit("Rustup stage fixture: " + message)


def read_pinned(path, expected_sha, expected_size, expected_mode=0o600):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        item = os.fstat(fd)
        if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink, item.st_size) != (0, 0, expected_mode, 1, expected_size):
            fail("pinned fixture input has unsafe metadata")
        chunks = []
        digest = hashlib.sha256()
        size = 0
        while True:
            block = os.read(fd, 1 << 20)
            if not block:
                break
            size += len(block)
            if size > expected_size:
                fail("pinned fixture input exceeded its size")
            digest.update(block)
            chunks.append(block)
        if size != expected_size or digest.hexdigest() != expected_sha:
            fail("pinned fixture input digest changed")
        return b"".join(chunks)
    finally:
        os.close(fd)


def load_stage(source):
    module = types.ModuleType("reviewed_rustup_stage")
    module.__file__ = STAGE
    exec(compile(source, STAGE, "exec"), module.__dict__)
    return module


def write_result(path, result):
    payload = (json.dumps(result, sort_keys=True, separators=(",", ":")) + "\n").encode()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        item = os.fstat(fd)
        if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink) != (0, 0, 0o600, 1):
            fail("private fixture result has unsafe metadata")
        offset = 0
        while offset < len(payload):
            count = os.write(fd, payload[offset:])
            if count <= 0:
                fail("short fixture result write")
            offset += count
        os.fsync(fd)
    finally:
        os.close(fd)


def main():
    if not sys.flags.isolated or not sys.flags.no_site:
        fail("run under Python -I -S")
    stage_item = os.stat(STAGE, follow_symlinks=False)
    source_bytes = read_pinned(STAGE, STAGE_SHA256, stage_item.st_size, 0o644)
    asset_bytes = read_pinned(ASSET, ASSET_SHA256, ASSET_SIZE)
    scratch = tempfile.mkdtemp(prefix="rootfs-rustup-fsize-fixture-v1-", dir=TASK)
    os.chmod(scratch, 0o700)
    rootfs = pathlib.Path(scratch, "rootfs")
    proxy = rootfs / "tools" / "rustup-proxy"
    proxy.mkdir(parents=True, mode=0o755)
    os.chmod(rootfs, 0o755)
    os.chmod(rootfs / "tools", 0o755)
    os.chmod(proxy, 0o755)

    stage = load_stage(source_bytes)
    stage.TASK = scratch
    stage.LOG_NAME = "stage-boundary.log"
    sys.argv = [STAGE, "--stage"]
    stage.start_boundary()
    limit = resource.getrlimit(resource.RLIMIT_FSIZE)
    if limit != (ASSET_SIZE, ASSET_SIZE):
        fail("reviewed stage boundary file-size limit differs")
    output = os.open(proxy / "rustup", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o555)
    try:
        stage.write_staged_rustup(output, asset_bytes)
    finally:
        os.close(output)
    copied = read_pinned(str(proxy / "rustup"), ASSET_SHA256, ASSET_SIZE, 0o555)
    result = {
        "asset_sha256": hashlib.sha256(copied).hexdigest(),
        "asset_size_bytes": len(copied),
        "copy_path": "rootfs-rustup-delta-v1.write_staged_rustup",
        "fsize_limit_bytes": limit[0],
        "status": "PASS",
    }
    write_result(os.path.join(scratch, "result.json"), result)


if __name__ == "__main__":
    main()

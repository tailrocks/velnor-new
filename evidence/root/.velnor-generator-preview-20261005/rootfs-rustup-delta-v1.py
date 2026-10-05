#!/usr/bin/python3
"""Stage one checksum-verified Rustup executable and its standard proxies."""

import ctypes
import datetime
import hashlib
import json
import os
import resource
import stat
import sys
import types

TASK = "/root/.velnor-generator-preview-20261005"
ROOTFS_NAME = "rootfs"
ASSET_NAME = "rustup-init-1.28.2-x86_64-unknown-linux-gnu"
CHECKSUM_NAME = ASSET_NAME + ".sha256"
PROVENANCE_NAME = "rustup-asset-provenance-v1.json"
BASE_MATERIALIZER_NAME = "preseed-preview-materialize.py"
INPUT_DELTA_NAME = "preseed-preview-rootfs-input-delta.py"
BASE_MANIFEST_NAME = "rootfs-base-manifest.json"
INPUT_DELTA_MANIFEST_NAME = "rootfs-input-delta-manifest.json"
INPUT_RESULT_NAME = "rootfs-input-delta-verify-result-v2.json"
LOG_NAME = "rootfs-rustup-delta-v1.log"
DELTA_MANIFEST_NAME = "rootfs-rustup-delta-v1-manifest.json"

EXPECTED_ASSET_SHA256 = "20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c"
EXPECTED_ASSET_SIZE = 20889000
EXPECTED_CHECKSUM_SHA256 = "fda213f7e5b0860abe98f2500ccef7cefd7a4c358833388a0a3d9fba43865fd8"
EXPECTED_PROVENANCE_SHA256 = "92be10601e7c4c8d6129b3c6463860f3eafe3cba928d5b901bae0de9226355bc"
EXPECTED_PYTHON_REALPATH = "/usr/bin/python3.13"
EXPECTED_PYTHON_SHA256 = "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9"
EXPECTED_PYTHON_SIZE = 6832784
EXPECTED_ENV_SHA256 = "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e"
EXPECTED_ENV_SIZE = 56144
EXPECTED_BASE_MATERIALIZER_SHA256 = "4aba299ea3cb925bae7413bd6b8b45845d1ce83a1f617d964b9068dbf87da0a3"
EXPECTED_INPUT_DELTA_SHA256 = "40e2acf6f621888e3a6f645dc86755d4f5e338d22b5ae9bba53b4f037967377a"
EXPECTED_BASE_MANIFEST_SHA256 = "1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf"
EXPECTED_INPUT_DELTA_MANIFEST_SHA256 = "dd5425fceb1524fc304e0da46cf6c64a8b63ee643f8ad6e6689cb15c241ef974"
EXPECTED_INPUT_RESULT_SHA256 = "08912633edeadfca709d587fc5b373824da2ee3cbedf0d2b02d6c85c873318f8"
EXPECTED_BASE_TREE_SHA256 = "1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763"
EXPECTED_INPUT_TREE_SHA256 = "117843b6eb5d4d06f2309fa205cc51e679445c1dc6f230461e4bd82aa7bf0684"
PROXY_NAMES = ("cargo", "cargo-clippy", "cargo-fmt", "clippy-driver", "rustc", "rustdoc", "rustfmt")


def fail(message):
    raise SystemExit("rootfs Rustup delta: " + message)


def verify_host_executable(path, expected_realpath, expected_sha256, expected_size):
    if os.path.realpath(path) != expected_realpath:
        fail("host executable path resolution changed: " + path)
    st = os.stat(expected_realpath, follow_symlinks=False)
    if not stat.S_ISREG(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o755 or st.st_size != expected_size:
        fail("host executable type, owner, mode, or size changed: " + path)
    fd = os.open(expected_realpath, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        digest = hashlib.sha256()
        size = 0
        while True:
            block = os.read(fd, 1 << 20)
            if not block:
                break
            size += len(block)
            if size > expected_size:
                fail("host executable exceeds its pinned size: " + path)
            digest.update(block)
        if size != expected_size or digest.hexdigest() != expected_sha256:
            fail("host executable digest changed: " + path)
    finally:
        os.close(fd)


def open_task_dir():
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        parts = TASK.split("/")[1:]
        for index, part in enumerate(parts):
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            st = os.fstat(child)
            if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0):
                os.close(child)
                fail("task path contains an unsafe directory")
            if index == len(parts) - 1:
                if stat.S_IMODE(st.st_mode) != 0o700:
                    os.close(child)
                    fail("task directory mode changed")
            elif st.st_mode & 0o022:
                os.close(child)
                fail("task ancestor is group or world writable")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def start_boundary():
    if sys.argv[1:] != ["--stage"]:
        fail("usage: rootfs-rustup-delta-v1.py --stage")
    if not sys.flags.isolated or not sys.flags.no_site:
        fail("start with Python -I -S")
    if sys.version_info[:3] != (3, 13, 5):
        fail("host Python version changed")
    verify_host_executable(sys.executable, EXPECTED_PYTHON_REALPATH, EXPECTED_PYTHON_SHA256, EXPECTED_PYTHON_SIZE)
    verify_host_executable("/usr/bin/env", "/usr/bin/env", EXPECTED_ENV_SHA256, EXPECTED_ENV_SIZE)
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("run only under the exact clean environment")
    if (os.geteuid(), os.getegid(), os.getgroups()) != (0, 0, [0]):
        fail("root staging requires only the root identity")
    if os.uname().nodename.split(".", 1)[0] != "bastion":
        fail("host identity mismatch")
    libc = ctypes.CDLL(None, use_errno=True)
    close_range = getattr(libc, "close_range", None)
    if close_range is None:
        fail("host libc has no close_range")
    close_range.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
    close_range.restype = ctypes.c_int
    if close_range(3, 0xFFFFFFFF, 0) != 0:
        fail("could not close inherited descriptors")
    task_fd = open_task_dir()
    log_fd = os.open(LOG_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)
    st = os.fstat(log_fd)
    if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600:
        os.close(log_fd)
        os.close(task_fd)
        fail("private stage log has an unsafe type, owner, or mode")
    null_fd = os.open("/dev/null", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    os.dup2(null_fd, 0)
    os.dup2(log_fd, 1)
    os.dup2(log_fd, 2)
    os.close(null_fd)
    os.close(log_fd)
    os.close(task_fd)
    resource.setrlimit(resource.RLIMIT_CPU, (120, 120))
    resource.setrlimit(resource.RLIMIT_FSIZE, (EXPECTED_ASSET_SIZE, EXPECTED_ASSET_SIZE))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_NPROC, (32, 32))
    resource.setrlimit(resource.RLIMIT_AS, (2 << 30, 2 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)


def read_private(task_fd, name, expected_sha, limit):
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600:
            fail("task input has an unsafe type, owner, or mode: " + name)
        if st.st_size > limit:
            fail("task input exceeds its size limit: " + name)
        digest = hashlib.sha256()
        chunks = []
        size = 0
        while True:
            block = os.read(fd, 1 << 20)
            if not block:
                break
            size += len(block)
            if size > limit:
                fail("task input exceeds its size limit: " + name)
            digest.update(block)
            chunks.append(block)
        if digest.hexdigest() != expected_sha:
            fail("task input digest changed: " + name)
        return b"".join(chunks)
    finally:
        os.close(fd)


def read_asset_file(task_fd, name, expected_sha, limit):
    assets_fd = os.open("assets", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
    try:
        assets_st = os.fstat(assets_fd)
        if not stat.S_ISDIR(assets_st.st_mode) or (assets_st.st_uid, assets_st.st_gid) != (0, 0) or stat.S_IMODE(assets_st.st_mode) != 0o700:
            fail("private asset directory has an unsafe owner or mode")
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=assets_fd)
    finally:
        os.close(assets_fd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600:
            fail("asset evidence has an unsafe type, owner, or mode: " + name)
        if st.st_size > limit:
            fail("asset evidence exceeds its size limit: " + name)
        digest = hashlib.sha256()
        chunks = []
        size = 0
        while True:
            block = os.read(fd, 1 << 20)
            if not block:
                break
            size += len(block)
            if size > limit:
                fail("asset evidence exceeds its size limit: " + name)
            digest.update(block)
            chunks.append(block)
        if digest.hexdigest() != expected_sha:
            fail("asset evidence digest changed: " + name)
        return b"".join(chunks)
    finally:
        os.close(fd)


def read_asset(task_fd):
    assets_fd = os.open("assets", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
    try:
        assets_st = os.fstat(assets_fd)
        if not stat.S_ISDIR(assets_st.st_mode) or (assets_st.st_uid, assets_st.st_gid) != (0, 0) or stat.S_IMODE(assets_st.st_mode) != 0o700:
            fail("private asset directory has an unsafe owner or mode")
        asset_fd = os.open(ASSET_NAME, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=assets_fd)
        try:
            st = os.fstat(asset_fd)
            if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600 or st.st_size != EXPECTED_ASSET_SIZE:
                fail("official Rustup asset type, owner, mode, or size changed")
            digest = hashlib.sha256()
            chunks = []
            while True:
                block = os.read(asset_fd, 1 << 20)
                if not block:
                    break
                digest.update(block)
                chunks.append(block)
            if digest.hexdigest() != EXPECTED_ASSET_SHA256:
                fail("official Rustup asset digest changed")
            return b"".join(chunks)
        finally:
            os.close(asset_fd)
    finally:
        os.close(assets_fd)


def load_reviewed_module(task_fd, filename, name, expected_sha):
    fd = os.open(filename, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o644 or st.st_size > (1 << 20):
            fail("rootfs verifier source has an unsafe type, owner, mode, or size: " + filename)
        digest = hashlib.sha256()
        chunks = []
        size = 0
        while True:
            block = os.read(fd, 1 << 16)
            if not block:
                break
            size += len(block)
            if size > (1 << 20):
                fail("rootfs verifier source exceeds its size limit: " + filename)
            digest.update(block)
            chunks.append(block)
        if digest.hexdigest() != expected_sha:
            fail("rootfs verifier source digest changed: " + filename)
        source = b"".join(chunks)
    finally:
        os.close(fd)
    module = types.ModuleType(name)
    module.__file__ = TASK + "/" + filename
    exec(compile(source, module.__file__, "exec"), module.__dict__)
    return module


def require_absent(task_fd, name):
    try:
        os.stat(name, dir_fd=task_fd, follow_symlinks=False)
    except FileNotFoundError:
        return
    fail("task output already exists: " + name)


def write_staged_rustup(file_fd, asset_bytes):
    view = memoryview(asset_bytes)
    while view:
        written = os.write(file_fd, view)
        if written <= 0:
            fail("short Rustup asset copy")
        view = view[written:]
    os.fchown(file_fd, 0, 0)
    os.fchmod(file_fd, 0o555)
    os.fsync(file_fd)
    written_st = os.fstat(file_fd)
    if not stat.S_ISREG(written_st.st_mode) or (written_st.st_uid, written_st.st_gid, stat.S_IMODE(written_st.st_mode), written_st.st_nlink, written_st.st_size) != (0, 0, 0o555, 1, EXPECTED_ASSET_SIZE):
        fail("staged Rustup executable identity changed")


def write_manifest(task_fd, data):
    fd = os.open(DELTA_MANIFEST_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)
    try:
        view = memoryview(data)
        while view:
            written = os.write(fd, view)
            if written <= 0:
                fail("short rootfs Rustup delta manifest write")
            view = view[written:]
        os.fsync(fd)
    finally:
        os.close(fd)


def main():
    start_boundary()
    task_fd = open_task_dir()
    try:
        base = load_reviewed_module(task_fd, BASE_MATERIALIZER_NAME, "reviewed_rootfs_base_materializer", EXPECTED_BASE_MATERIALIZER_SHA256)
        delta = load_reviewed_module(task_fd, INPUT_DELTA_NAME, "reviewed_rootfs_input_delta", EXPECTED_INPUT_DELTA_SHA256)
        base_manifest = read_private(task_fd, BASE_MANIFEST_NAME, EXPECTED_BASE_MANIFEST_SHA256, 4096)
        input_delta_manifest_bytes = read_private(task_fd, INPUT_DELTA_MANIFEST_NAME, EXPECTED_INPUT_DELTA_MANIFEST_SHA256, 4096)
        input_result = read_private(task_fd, INPUT_RESULT_NAME, EXPECTED_INPUT_RESULT_SHA256, 4096)
        require_absent(task_fd, DELTA_MANIFEST_NAME)
        provenance = read_asset_file(task_fd, PROVENANCE_NAME, EXPECTED_PROVENANCE_SHA256, 4096)
        checksum = read_asset_file(task_fd, CHECKSUM_NAME, EXPECTED_CHECKSUM_SHA256, 4096)
        official_digest = (checksum.split(None, 1)[0]).decode("ascii")
        if checksum != (EXPECTED_ASSET_SHA256 + " *./rustup-init\n").encode() or official_digest != EXPECTED_ASSET_SHA256:
            fail("official Rustup checksum sidecar differs")
        provenance_json = json.loads(provenance)
        expected_provenance = {
            "schema": 1,
            "status": "OFFICIAL_ASSET_DOWNLOADED_AND_CHECKSUM_VERIFIED; NOT_EXECUTED",
            "project": "rust-lang/rustup",
            "version": "1.28.2",
            "target": "x86_64-unknown-linux-gnu",
            "asset_name": "rustup-init",
            "asset_url": "https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-unknown-linux-gnu/rustup-init",
            "checksum_url": "https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-unknown-linux-gnu/rustup-init.sha256",
            "official_sha256": EXPECTED_ASSET_SHA256,
            "asset_size_bytes": EXPECTED_ASSET_SIZE,
            "checksum_sidecar_sha256": EXPECTED_CHECKSUM_SHA256,
            "checksum_sidecar_text": "20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c *./rustup-init\n",
        }
        if any(provenance_json.get(key) != value for key, value in expected_provenance.items()):
            fail("Rustup asset provenance fields differ from the reviewed official asset")
        if json.loads(base_manifest).get("base_rootfs_tree_sha256") != EXPECTED_BASE_TREE_SHA256:
            fail("sealed rootfs base manifest changed")
        input_manifest = json.loads(input_delta_manifest_bytes)
        if input_manifest.get("base_manifest_sha256") != EXPECTED_BASE_MANIFEST_SHA256 or input_manifest.get("result_rootfs_tree_sha256") != EXPECTED_INPUT_TREE_SHA256:
            fail("empty-input rootfs delta manifest changed")
        if json.loads(input_result).get("result_tree_sha256") != EXPECTED_INPUT_TREE_SHA256:
            fail("rootfs input verification result changed")

        asset_bytes = read_asset(task_fd)
        root_fd = os.open(ROOTFS_NAME, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
        try:
            before_records = delta.tree_records(root_fd)
            before_tree = delta.digest_records(before_records)
            if base.tree_hash(TASK + "/" + ROOTFS_NAME) != EXPECTED_INPUT_TREE_SHA256 or before_tree != EXPECTED_INPUT_TREE_SHA256:
                fail("rootfs does not match the reviewed base plus empty-input delta")
            tools_fd = os.open("tools", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=root_fd)
            try:
                try:
                    os.stat("rustup-proxy", dir_fd=tools_fd, follow_symlinks=False)
                except FileNotFoundError:
                    pass
                else:
                    fail("Rustup proxy destination already exists")
                os.mkdir("rustup-proxy", 0o755, dir_fd=tools_fd)
                proxy_fd = os.open("rustup-proxy", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=tools_fd)
                try:
                    proxy_st = os.fstat(proxy_fd)
                    if (proxy_st.st_uid, proxy_st.st_gid) != (0, 0):
                        fail("new Rustup proxy directory owner changed")
                    if stat.S_IMODE(proxy_st.st_mode) != 0o755:
                        os.fchmod(proxy_fd, 0o755)
                    file_fd = os.open("rustup", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o555, dir_fd=proxy_fd)
                    try:
                        write_staged_rustup(file_fd, asset_bytes)
                    finally:
                        os.close(file_fd)
                    for name in PROXY_NAMES:
                        os.symlink("rustup", name, dir_fd=proxy_fd)
                    os.fsync(proxy_fd)
                finally:
                    os.close(proxy_fd)
                os.fsync(tools_fd)
            finally:
                os.close(tools_fd)
            after_records = delta.tree_records(root_fd)
            prefix = "/tools/rustup-proxy"
            expected_new = [
                {"path": prefix, "type": "directory", "mode": 0o755, "uid": 0, "gid": 0},
            ] + [
                {"path": prefix + "/" + name, "type": "symlink", "mode": 0o777, "uid": 0, "gid": 0, "target": "rustup"}
                for name in PROXY_NAMES
            ] + [{"path": prefix + "/rustup", "type": "file", "mode": 0o555, "uid": 0, "gid": 0, "size": EXPECTED_ASSET_SIZE, "sha256": EXPECTED_ASSET_SHA256}]
            if [record for record in after_records if record["path"] == prefix or record["path"].startswith(prefix + "/")] != expected_new:
                fail("rootfs Rustup proxy additions differ from the reviewed path contract")
            if [record for record in after_records if not (record["path"] == prefix or record["path"].startswith(prefix + "/"))] != before_records:
                fail("rootfs changed beyond the Rustup proxy directory")
            result_tree = delta.digest_records(after_records)
        finally:
            os.close(root_fd)

        document = {
            "schema": 1,
            "status": "RUSTUP_1_28_2_OFFICIAL_ASSET_STAGED; NOT_EXECUTED; cargo seed and full rootfs seal pending",
            "parent_rootfs_tree_sha256": EXPECTED_INPUT_TREE_SHA256,
            "parent_input_delta_manifest_sha256": EXPECTED_INPUT_DELTA_MANIFEST_SHA256,
            "parent_input_verification_result_sha256": EXPECTED_INPUT_RESULT_SHA256,
            "asset_provenance_sha256": EXPECTED_PROVENANCE_SHA256,
            "asset_sha256": EXPECTED_ASSET_SHA256,
            "asset_size_bytes": EXPECTED_ASSET_SIZE,
            "asset_version": "1.28.2",
            "asset_target": "x86_64-unknown-linux-gnu",
            "asset_role": "rustup-init official binary staged as rustup proxy; not executed by root",
            "staging_runtime": {
                "env_sha256": EXPECTED_ENV_SHA256,
                "python_realpath": EXPECTED_PYTHON_REALPATH,
                "python_sha256": EXPECTED_PYTHON_SHA256,
                "python_version": "3.13.5",
            },
            "added_paths": expected_new,
            "result_rootfs_tree_sha256": result_tree,
            "execution": "No Rustup, installer, Cargo, Mise, compiler, Git, network, container, or account command ran during staging.",
            "reversal": "Remove only the exact seven symlinks, rustup binary, and empty /tools/rustup-proxy directory after verifying this manifest and the staged hashes.",
            "timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        }
        manifest_bytes = (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode()
        write_manifest(task_fd, manifest_bytes)
        print("PASS: staged verified Rustup proxy bytes without execution; result_tree=" + result_tree + " delta_manifest_sha256=" + hashlib.sha256(manifest_bytes).hexdigest())
    finally:
        os.close(task_fd)


if __name__ == "__main__":
    main()

#!/usr/bin/python3
"""Exercise the pinned Python runtime and managed-home helper in a scratch chroot."""

import ctypes
import hashlib
import json
import os
import pathlib
import resource
import shutil
import stat
import subprocess
import sys
import tempfile

TASK = "/root/.velnor-generator-preview-20261005"
ROOTFS = TASK + "/rootfs"
INVENTORY = TASK + "/python-runtime-inventory-v1.json"
DELTA_SCRIPT = TASK + "/rootfs-python-runtime-delta-v1.py"
HELPER = TASK + "/rustup-home-prepare-v3.py"
TREE_MODULE = TASK + "/preseed-preview-rootfs-input-delta.py"
RESULT_NAME = "rootfs-python-runtime-fixture-v1-result.json"
LOG_NAME = "rootfs-python-runtime-fixture-v1.log"
OUTPUT_NAME = "rootfs-python-runtime-fixture-v1-output.log"
EXPECTED_ENV_SHA256 = "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e"
EXPECTED_ENV_SIZE = 56144
EXPECTED_INVENTORY_SHA256 = "a309c30a1016b3dc03eb8cbb043a21186856b20a1085c88ce662926c3ca00dfe"
EXPECTED_DELTA_SCRIPT_SHA256 = "98275ae5319647a0724d449d76340e3f16060a186034c3ce44bccc69c400ffd1"
EXPECTED_HELPER_SHA256 = "bf2ff3dbf7386eb7d3713edf381ec17e1bd81cd48414fe7ebefd77dd17f86250"
EXPECTED_TREE_MODULE_SHA256 = "40e2acf6f621888e3a6f645dc86755d4f5e338d22b5ae9bba53b4f037967377a"
EXPECTED_ROOTFS_TREE_SHA256 = "fa4194fa10eecd66c72580c4c6acde1d02a1a9a800275b15178ae5b4bf6acf06"
EXPECTED_ENV_SHA256 = "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e"
EXPECTED_PYTHON_SHA256 = "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9"
EXPECTED_PYTHON_SIZE = 6832784
UID = 65534
GID = 65534

DEPENDENCIES = (
    "/lib/x86_64-linux-gnu/libc.so.6",
    "/lib/x86_64-linux-gnu/libm.so.6",
    "/lib/x86_64-linux-gnu/libz.so.1",
    "/lib64/ld-linux-x86-64.so.2",
)

CHILD_CODE = r'''import hashlib, json, os, sys
assert sys.flags.isolated and sys.flags.no_site and sys.flags.dont_write_bytecode
assert sys.version_info[:3] == (3, 13, 5), sys.version
assert (os.geteuid(), os.getegid(), os.getgroups()) == (65534, 65534, [])
assert hashlib.sha256(b"fallback-check").hexdigest() == "0fe830c65f08f9e0f827ba96c32e373016115f4f15ae213057cd347e20c1d511"
assert "_hashlib" not in sys.modules
try:
    import _hashlib
except ImportError:
    pass
else:
    raise AssertionError("OpenSSL-backed _hashlib unexpectedly loaded")
source_path = "/work/rustup-home-prepare-v3.py"
with open(source_path, "rb") as source_file:
    source = source_file.read(1 << 20)
namespace = {"__name__": "runtime_fixture_helper", "__file__": source_path}
exec(compile(source, source_path, "exec"), namespace)
fake_sha = "FAKE_RUSTUP_SHA256"
first = namespace["prepare"]("/tools", "/work", 65534, 65534, fake_sha, require_caller=True)
second = namespace["prepare"]("/tools", "/work", 65534, 65534, fake_sha, require_caller=True)
assert first == second and first["status"] == "PASS"
print(json.dumps({"python": sys.version.split()[0], "sha256_backend": "built-in _sha2 fallback", "openssl_extension": "absent", "helper_prepare": "PASS", "helper_repeat": "PASS", "status": "PASS"}, sort_keys=True))
'''


def fail(message):
    raise SystemExit("Python runtime fixture: " + message)


def digest_file(path, limit):
    fd = open_source(path)
    try:
        item = os.fstat(fd)
        if not stat.S_ISREG(item.st_mode) or item.st_nlink != 1 or item.st_size > limit:
            fail("unsafe input file: " + path)
        digest = hashlib.sha256()
        size = 0
        chunks = []
        while True:
            block = os.read(fd, 1 << 20)
            if not block:
                break
            size += len(block)
            if size > limit:
                fail("input file exceeds its limit: " + path)
            digest.update(block)
            chunks.append(block)
        return digest.hexdigest(), b"".join(chunks), item
    finally:
        os.close(fd)


def open_abs_dir(path):
    if not path.startswith("/") or "." in path.split("/") or ".." in path.split("/"):
        fail("unsafe absolute path")
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        for part in path.split("/")[1:]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            item = os.fstat(child)
            if not stat.S_ISDIR(item.st_mode) or item.st_uid != 0 or item.st_gid != 0 or item.st_mode & 0o022:
                os.close(child)
                fail("unsafe path ancestor")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def open_source(path):
    parts = path.split("/")[1:]
    if not path.startswith("/") or not parts or any(part in ("", ".", "..") for part in parts):
        fail("unsafe source path")
    parent = open_abs_dir("/" + "/".join(parts[:-1])) if len(parts) > 1 else os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        fd = os.open(parts[-1], os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
    finally:
        os.close(parent)
    return fd


def copy_bytes(root, path, data, mode):
    dest = root / path.lstrip("/")
    dest.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
    fd = os.open(dest, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, mode)
    try:
        view = memoryview(data)
        while view:
            count = os.write(fd, view)
            if count <= 0:
                fail("short scratch file write")
            view = view[count:]
        os.fchown(fd, 0, 0)
        os.fchmod(fd, mode)
        os.fsync(fd)
    finally:
        os.close(fd)


def copy_rootfs_file(root, root_fd, record):
    path = record["path"]
    parts = path.split("/")[1:]
    parent = os.dup(root_fd)
    try:
        for part in parts[:-1]:
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
            os.close(parent)
            parent = child
        source_fd = os.open(parts[-1], os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
        try:
            item = os.fstat(source_fd)
            if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink, item.st_size) != (0, 0, record["mode"], 1, record["size"]):
                fail("sealed dependency identity changed: " + path)
            data = bytearray()
            digest = hashlib.sha256()
            while True:
                block = os.read(source_fd, 1 << 20)
                if not block:
                    break
                data.extend(block)
                digest.update(block)
            if digest.hexdigest() != record["sha256"] or len(data) != record["size"]:
                fail("sealed dependency hash changed: " + path)
        finally:
            os.close(source_fd)
    finally:
        os.close(parent)
    copy_bytes(root, path, bytes(data), record["mode"])


def make_dirs(root, inventory):
    paths = {"/usr", "/lib", "/lib/x86_64-linux-gnu", "/lib64", "/tmp", "/tools", "/work"}
    for entry in inventory["dirs"]:
        paths.add(entry["path"])
    for path in sorted(paths, key=lambda item: (item.count("/"), item)):
        if path == "/tmp":
            mode = 0o1777
        elif path == "/work":
            mode = 0o700
        else:
            mode = 0o755
        target = root / path.lstrip("/")
        target.mkdir(mode=mode, exist_ok=True, parents=False)
        os.chmod(target, mode)
        os.chown(target, 0, 0)


def prepare_fake_tools(root, helper_bytes):
    tools = root / "tools"
    (tools / "rustup-proxy").mkdir(mode=0o755)
    (tools / "rust-1.98.1" / "bin").mkdir(mode=0o755, parents=True)
    (tools / "mise-data" / "installs" / "rust").mkdir(mode=0o755, parents=True)
    fake_rustup = b"synthetic runtime fixture Rustup; never executed\n"
    rustup = tools / "rustup-proxy" / "rustup"
    rustup.write_bytes(fake_rustup)
    os.chmod(rustup, 0o555)
    for name in ("cargo", "cargo-clippy", "cargo-fmt", "clippy-driver", "rustc", "rustdoc", "rustfmt"):
        (tools / "rustup-proxy" / name).symlink_to("rustup")
    for name in ("cargo", "rustc"):
        path = tools / "rust-1.98.1" / "bin" / name
        path.write_text("synthetic " + name + " identity; never executed\n")
        os.chmod(path, 0o755)
    (tools / "mise-data" / "installs" / "rust" / "1.98.1").symlink_to("/tools/rust-1.98.1")
    work = root / "work"
    helper = work / "rustup-home-prepare-v3.py"
    helper.write_bytes(helper_bytes)
    os.chown(helper, 0, 0)
    os.chmod(helper, 0o444)
    for subdir in ("cargo-seed", "rustup-home", "mise-data"):
        path = work / subdir
        path.mkdir(mode=0o700)
        os.chown(path, UID, GID)
        os.chmod(path, 0o700)
    os.chown(work, UID, GID)
    os.chmod(work, 0o700)
    for path in [tools, *tools.rglob("*")]:
        if path.is_symlink():
            os.chown(path, 0, 0, follow_symlinks=False)
        else:
            os.chown(path, 0, 0)
            if path.is_dir():
                os.chmod(path, 0o755)
    for path in (root / "tmp",):
        os.chmod(path, 0o1777)
        os.chown(path, 0, 0)
    return hashlib.sha256(fake_rustup).hexdigest()


def child_setup(root):
    libc = ctypes.CDLL(None, use_errno=True)
    unshare = libc.unshare
    unshare.argtypes = [ctypes.c_int]
    unshare.restype = ctypes.c_int
    if unshare(0x40000000) != 0:  # CLONE_NEWNET
        err = ctypes.get_errno()
        raise OSError(err, os.strerror(err), "unshare(CLONE_NEWNET)")
    os.chroot(str(root))
    os.chdir("/")
    os.setgroups([])
    os.setgid(GID)
    os.setuid(UID)
    prctl = libc.prctl
    prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]
    prctl.restype = ctypes.c_int
    if prctl(38, 1, 0, 0, 0) != 0:  # PR_SET_NO_NEW_PRIVS
        err = ctypes.get_errno()
        raise OSError(err, os.strerror(err), "prctl(PR_SET_NO_NEW_PRIVS)")
    resource.setrlimit(resource.RLIMIT_CPU, (30, 30))
    resource.setrlimit(resource.RLIMIT_FSIZE, (1 << 20, 1 << 20))
    resource.setrlimit(resource.RLIMIT_NOFILE, (32, 32))
    resource.setrlimit(resource.RLIMIT_NPROC, (16, 16))
    resource.setrlimit(resource.RLIMIT_AS, (512 << 20, 512 << 20))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def write_private(name, data):
    parent = open_abs_dir(TASK)
    try:
        fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=parent)
    except BaseException:
        os.close(parent)
        raise
    try:
        view = memoryview(data)
        while view:
            count = os.write(fd, view)
            if count <= 0:
                fail("short evidence write")
            view = view[count:]
        os.fsync(fd)
    finally:
        os.close(fd)
        os.close(parent)


def close_inherited_fds():
    libc = ctypes.CDLL(None, use_errno=True)
    close_range = getattr(libc, "close_range", None)
    if close_range is None:
        fail("host libc has no close_range")
    close_range.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
    close_range.restype = ctypes.c_int
    if close_range(3, 0xFFFFFFFF, 0) != 0:
        fail("could not close inherited descriptors")


def start_boundary():
    if not sys.flags.isolated or not sys.flags.no_site or not sys.flags.dont_write_bytecode:
        fail("start with Python -I -S -B")
    if sys.version_info[:3] != (3, 13, 5) or os.path.realpath(sys.executable) != "/usr/bin/python3.13":
        fail("host Python identity changed")
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("run only under the exact clean environment")
    if (os.geteuid(), os.getegid(), os.getgroups()) != (0, 0, [0]):
        fail("fixture requires root only for private chroot setup")
    if os.uname().nodename.split(".", 1)[0] != "bastion":
        fail("host identity mismatch")

    env_sha, _, env_stat = digest_file("/usr/bin/env", 100000)
    python_sha, _, python_stat = digest_file("/usr/bin/python3.13", 10 << 20)
    if env_sha != EXPECTED_ENV_SHA256 or (env_stat.st_uid, env_stat.st_gid, stat.S_IMODE(env_stat.st_mode), env_stat.st_size, env_stat.st_nlink) != (0, 0, 0o755, EXPECTED_ENV_SIZE, 1):
        fail("host env executable identity changed")
    if python_sha != EXPECTED_PYTHON_SHA256 or (python_stat.st_uid, python_stat.st_gid, stat.S_IMODE(python_stat.st_mode), python_stat.st_size, python_stat.st_nlink) != (0, 0, 0o755, EXPECTED_PYTHON_SIZE, 1):
        fail("host Python executable identity changed")

    task_fd = open_abs_dir(TASK)
    try:
        task_stat = os.fstat(task_fd)
        if (task_stat.st_uid, task_stat.st_gid, stat.S_IMODE(task_stat.st_mode)) != (0, 0, 0o700):
            fail("task directory ownership or mode changed")
        for name in (RESULT_NAME, LOG_NAME, OUTPUT_NAME):
            try:
                os.stat(name, dir_fd=task_fd, follow_symlinks=False)
            except FileNotFoundError:
                continue
            fail("fixture output already exists: " + name)
        output_fd = os.open(OUTPUT_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)
        output_stat = os.fstat(output_fd)
        if not stat.S_ISREG(output_stat.st_mode) or (output_stat.st_uid, output_stat.st_gid, stat.S_IMODE(output_stat.st_mode), output_stat.st_nlink) != (0, 0, 0o600, 1):
            os.close(output_fd)
            fail("private fixture output log has an unsafe identity")
    finally:
        os.close(task_fd)

    null_fd = os.open("/dev/null", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    null_stat = os.fstat(null_fd)
    if not stat.S_ISCHR(null_stat.st_mode) or (null_stat.st_uid, null_stat.st_gid, stat.S_IMODE(null_stat.st_mode)) != (0, 0, 0o666):
        os.close(null_fd)
        os.close(output_fd)
        fail("host null device identity changed")
    os.dup2(null_fd, 0)
    os.dup2(output_fd, 1)
    os.dup2(output_fd, 2)
    os.close(null_fd)
    os.close(output_fd)
    close_inherited_fds()
    resource.setrlimit(resource.RLIMIT_CPU, (60, 60))
    resource.setrlimit(resource.RLIMIT_FSIZE, (16 << 20, 16 << 20))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_NPROC, (32, 32))
    resource.setrlimit(resource.RLIMIT_AS, (1 << 30, 1 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)
    return env_sha, python_sha


def load_delta_namespace(delta_bytes):
    namespace = {"__name__": "runtime_fixture_delta", "__file__": DELTA_SCRIPT}
    exec(compile(delta_bytes, DELTA_SCRIPT, "exec"), namespace)
    return namespace


def exercise_stage_output_guard(scratch, delta_namespace):
    guard = scratch / "stage-output-guard"
    guard.mkdir(mode=0o700)
    guard_fd = os.open(guard, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    log_name = delta_namespace["LOG_NAME"]
    manifest_name = delta_namespace["MANIFEST_NAME"]
    try:
        log_fd = delta_namespace["open_stage_log"](guard_fd)
        log_stat = os.fstat(log_fd)
        if not stat.S_ISREG(log_stat.st_mode) or (log_stat.st_uid, log_stat.st_gid, stat.S_IMODE(log_stat.st_mode), log_stat.st_nlink) != (0, 0, 0o600, 1):
            fail("stage log guard created an unsafe file")
        os.close(log_fd)
        try:
            delta_namespace["open_stage_log"](guard_fd)
        except SystemExit:
            pass
        else:
            fail("stage log guard accepted an existing log")
        os.unlink(log_name, dir_fd=guard_fd)

        manifest_fd = os.open(manifest_name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=guard_fd)
        sentinel = b"unchanged-stage-manifest-sentinel\n"
        os.write(manifest_fd, sentinel)
        os.fsync(manifest_fd)
        os.close(manifest_fd)
        try:
            delta_namespace["open_stage_log"](guard_fd)
        except SystemExit:
            pass
        else:
            fail("stage log guard accepted an existing manifest")
        fd = os.open(manifest_name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=guard_fd)
        try:
            if os.read(fd, len(sentinel) + 1) != sentinel:
                fail("stage log guard changed the existing manifest")
        finally:
            os.close(fd)
        os.unlink(manifest_name, dir_fd=guard_fd)
    finally:
        os.close(guard_fd)
    return True


def main():
    if sys.argv[1:] != ["--run"]:
        fail("usage: rootfs-python-runtime-fixture-v1.py --run")
    env_sha, python_sha = start_boundary()
    inventory_sha, inventory_bytes, inventory_st = digest_file(INVENTORY, 200000)
    helper_sha, helper_bytes, helper_st = digest_file(HELPER, 1 << 20)
    delta_sha, delta_bytes, delta_st = digest_file(DELTA_SCRIPT, 1 << 20)
    tree_sha, _, tree_st = digest_file(TREE_MODULE, 1 << 20)
    expected_input = (
        (INVENTORY, inventory_sha, inventory_st, EXPECTED_INVENTORY_SHA256, len(inventory_bytes), 0o600),
        (HELPER, helper_sha, helper_st, EXPECTED_HELPER_SHA256, 15684, 0o444),
        (DELTA_SCRIPT, delta_sha, delta_st, EXPECTED_DELTA_SCRIPT_SHA256, len(delta_bytes), 0o444),
        (TREE_MODULE, tree_sha, tree_st, EXPECTED_TREE_MODULE_SHA256, tree_st.st_size, 0o644),
    )
    for path, got_sha, item, wanted_sha, wanted_size, wanted_mode in expected_input:
        if got_sha != wanted_sha or item.st_size != wanted_size or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink) != (0, 0, wanted_mode, 1):
            fail("reviewed input identity changed: " + path)
    inventory = json.loads(inventory_bytes)

    tree_spec = __import__("importlib.util", fromlist=["spec_from_file_location"])
    spec = tree_spec.spec_from_file_location("verified_tree", TREE_MODULE)
    tree = tree_spec.module_from_spec(spec)
    spec.loader.exec_module(tree)
    delta_namespace = load_delta_namespace(delta_bytes)
    delta_namespace["check_inventory"](inventory)
    root_fd = open_abs_dir(ROOTFS)
    try:
        records = tree.tree_records(root_fd)
        tree_digest = tree.digest_records(records)
        if tree_digest != EXPECTED_ROOTFS_TREE_SHA256:
            fail("sealed rootfs dependency tree changed")
        by_path = {entry["path"]: entry for entry in records}
        if len(by_path) != len(records):
            fail("sealed rootfs has duplicate tree paths")
        dependency_records = {}
        for path in DEPENDENCIES:
            entry = by_path.get(path)
            if not entry or entry.get("type") != "file":
                fail("sealed rootfs runtime dependency missing: " + path)
            dependency_records[path] = entry
    finally:
        os.close(root_fd)

    result_fd = open_abs_dir(TASK)
    try:
        for name in (RESULT_NAME, LOG_NAME):
            try:
                os.stat(name, dir_fd=result_fd, follow_symlinks=False)
            except FileNotFoundError:
                continue
            fail("fixture output already exists: " + name)
    finally:
        os.close(result_fd)

    scratch = pathlib.Path(tempfile.mkdtemp(prefix="rootfs-python-runtime-fixture-v1-", dir=TASK))
    os.chmod(scratch, 0o700)
    os.chown(scratch, 0, 0)
    status = "FAIL"
    output = b""
    stderr = b""
    cleanup = "PRESERVED_ON_FAILURE"
    try:
        stage_output_guard = exercise_stage_output_guard(scratch, delta_namespace)
        make_dirs(scratch, inventory)
        for entry in inventory["files"]:
            if entry["kind"] == "file":
                source_fd = open_source(entry["source"])
                try:
                    source_st = os.fstat(source_fd)
                    if not stat.S_ISREG(source_st.st_mode) or (source_st.st_uid, source_st.st_gid, stat.S_IMODE(source_st.st_mode), source_st.st_nlink, source_st.st_size) != (entry["uid"], entry["gid"], entry["mode"], 1, entry["size"]):
                        fail("runtime source file identity changed: " + entry["source"])
                    data = bytearray()
                    digest = hashlib.sha256()
                    while True:
                        block = os.read(source_fd, 1 << 20)
                        if not block:
                            break
                        data.extend(block)
                        digest.update(block)
                    if digest.hexdigest() != entry["sha256"] or len(data) != entry["size"]:
                        fail("runtime source file hash changed: " + entry["source"])
                finally:
                    os.close(source_fd)
                copy_bytes(scratch, entry["destination"], bytes(data), entry["mode"])
            else:
                source_parts = entry["source"].split("/")[1:]
                source_parent_fd = open_abs_dir("/" + "/".join(source_parts[:-1]))
                try:
                    source_st = os.stat(source_parts[-1], dir_fd=source_parent_fd, follow_symlinks=False)
                    if not stat.S_ISLNK(source_st.st_mode) or (source_st.st_uid, source_st.st_gid, stat.S_IMODE(source_st.st_mode), source_st.st_nlink) != (entry["uid"], entry["gid"], entry["mode"], 1) or os.readlink(source_parts[-1], dir_fd=source_parent_fd) != entry["target"]:
                        fail("runtime source symlink identity changed: " + entry["source"])
                finally:
                    os.close(source_parent_fd)
                destination = scratch / entry["destination"].lstrip("/")
                destination.symlink_to(entry["target"])
        dependencies_fd = open_abs_dir(ROOTFS)
        try:
            for path, record in dependency_records.items():
                copy_rootfs_file(scratch, dependencies_fd, record)
        finally:
            os.close(dependencies_fd)
        fake_sha = prepare_fake_tools(scratch, helper_bytes)

        child_code = CHILD_CODE.replace("FAKE_RUSTUP_SHA256", fake_sha)
        process = subprocess.run(
            ["/usr/bin/python3", "-I", "-S", "-B", "-c", child_code],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd="/",
            env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"},
            close_fds=True,
            preexec_fn=lambda: child_setup(scratch),
            timeout=45,
            check=False,
        )
        output, stderr = process.stdout, process.stderr
        if process.returncode != 0:
            fail("scratch-chroot Python/helper run failed with status " + str(process.returncode))
        decoded = json.loads(output)
        if decoded.get("status") != "PASS" or decoded.get("helper_prepare") != "PASS" or decoded.get("helper_repeat") != "PASS" or decoded.get("openssl_extension") != "absent":
            fail("scratch-chroot fixture output contract failed")
        status = "PASS"
        shutil.rmtree(scratch)
        cleanup = "PASS; only private fixture scratch removed"
    except BaseException:
        raise
    finally:
        log = {
            "status": status,
            "scratch_path": str(scratch),
            "stdout": output.decode("utf-8", "replace"),
            "stderr": stderr.decode("utf-8", "replace"),
            "scratch_cleanup": cleanup,
        }
        write_private(LOG_NAME, (json.dumps(log, sort_keys=True, separators=(",", ":")) + "\n").encode())
    record = {
        "schema": 1,
        "status": status,
        "inventory_sha256": inventory_sha,
        "delta_script_sha256": delta_sha,
        "helper_sha256": helper_sha,
        "rootfs_tree_sha256": tree_digest,
        "python_sha256": python_sha,
        "python_version": "3.13.5",
        "environment": {"PATH": "/usr/bin:/bin", "LC_ALL": "C", "env_sha256": env_sha},
        "child": json.loads(output) if status == "PASS" else None,
        "stage_output_guard": "PASS" if stage_output_guard else "FAIL",
        "network": "private network namespace; no external access",
        "identity": "UID/GID 65534; no supplementary groups; no_new_privs",
        "tools": "synthetic Rustup/Cargo/Rustc records; none executed",
        "cleanup": cleanup,
        "timestamp_utc": __import__("datetime").datetime.now(__import__("datetime").timezone.utc).isoformat(),
    }
    write_private(RESULT_NAME, (json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n").encode())
    print(json.dumps(record, sort_keys=True))


if __name__ == "__main__":
    main()

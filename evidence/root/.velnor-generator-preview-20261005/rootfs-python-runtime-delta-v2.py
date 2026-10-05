#!/usr/bin/python3
"""Stage the pinned Python helper runtime into the private generator rootfs."""

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
INVENTORY_NAME = "python-runtime-inventory-v1.json"
PARENT_MANIFEST_NAME = "rootfs-rustup-delta-v1-manifest.json"
INPUT_MANIFEST_NAME = "rootfs-input-delta-manifest.json"
INPUT_RESULT_NAME = "rootfs-input-delta-verify-result-v2.json"
TREE_MODULE_NAME = "preseed-preview-rootfs-input-delta.py"
HELPER_NAME = "rustup-home-prepare-v3.py"
LOG_NAME = "rootfs-python-runtime-delta-v2.log"
MANIFEST_NAME = "rootfs-python-runtime-delta-v2-manifest.json"

EXPECTED_PYTHON = "/usr/bin/python3.13"
EXPECTED_PYTHON_SHA256 = "889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9"
EXPECTED_PYTHON_SIZE = 6832784
EXPECTED_ENV_SHA256 = "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e"
EXPECTED_ENV_SIZE = 56144
EXPECTED_INVENTORY_SHA256 = "a309c30a1016b3dc03eb8cbb043a21186856b20a1085c88ce662926c3ca00dfe"
EXPECTED_PARENT_MANIFEST_SHA256 = "34cac9f1449601a9c2ddbf526bd4b08724cad30677785c5b445007d9736fcf7d"
EXPECTED_INPUT_MANIFEST_SHA256 = "dd5425fceb1524fc304e0da46cf6c64a8b63ee643f8ad6e6689cb15c241ef974"
EXPECTED_INPUT_RESULT_SHA256 = "08912633edeadfca709d587fc5b373824da2ee3cbedf0d2b02d6c85c873318f8"
EXPECTED_TREE_MODULE_SHA256 = "40e2acf6f621888e3a6f645dc86755d4f5e338d22b5ae9bba53b4f037967377a"
EXPECTED_HELPER_SHA256 = "bf2ff3dbf7386eb7d3713edf381ec17e1bd81cd48414fe7ebefd77dd17f86250"
EXPECTED_PARENT_TREE_SHA256 = "fa4194fa10eecd66c72580c4c6acde1d02a1a9a800275b15178ae5b4bf6acf06"
EXPECTED_INPUT_TREE_SHA256 = "117843b6eb5d4d06f2309fa205cc51e679445c1dc6f230461e4bd82aa7bf0684"

PYTHON_VERSION = "3.13.5"
PYTHON_PACKAGE = "libpython3.13-minimal:amd64\t3.13.5-2+deb13u5"
INTERPRETER_PACKAGE = "python3.13-minimal\t3.13.5-2+deb13u5"
PYTHON_LINK_PACKAGE = "python3-minimal\t3.13.5-1"
EXPAT_PACKAGE = "libexpat1:amd64\t2.8.3-1~deb13u1"
EXPECTED_FILE_COUNT = 287
EXPECTED_DIRECTORY_COUNT = 20
EXPECTED_SYMLINK_COUNT = 3
EXPECTED_REGULAR_BYTES = 12032961
MAX_INVENTORY_BYTES = 200000
MAX_SINGLE_FILE_BYTES = 8 << 20


def fail(message):
    raise SystemExit("rootfs Python runtime delta: " + message)


def open_stage_log(task_fd):
    for name in (LOG_NAME, MANIFEST_NAME):
        try:
            os.stat(name, dir_fd=task_fd, follow_symlinks=False)
        except FileNotFoundError:
            continue
        fail("task output already exists: " + name)
    return os.open(LOG_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)


def hash_bytes(data):
    return hashlib.sha256(data).hexdigest()


def verify_host_executable(path, expected_realpath, expected_sha256, expected_size):
    if os.path.realpath(path) != expected_realpath:
        fail("host executable path resolution changed: " + path)
    fd = os.open(expected_realpath, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        item = os.fstat(fd)
        if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid) != (0, 0) or stat.S_IMODE(item.st_mode) != 0o755 or item.st_size != expected_size or item.st_nlink != 1:
            fail("host executable type, owner, mode, link count, or size changed: " + path)
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
            item = os.fstat(child)
            if not stat.S_ISDIR(item.st_mode) or (item.st_uid, item.st_gid) != (0, 0):
                os.close(child)
                fail("task path contains an unsafe directory")
            if index == len(parts) - 1:
                if stat.S_IMODE(item.st_mode) != 0o700:
                    os.close(child)
                    fail("task directory mode changed")
            elif item.st_mode & 0o022:
                os.close(child)
                fail("task ancestor is group/world writable")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def start_boundary():
    if sys.argv[1:] != ["--stage"]:
        fail("usage: rootfs-python-runtime-delta-v2.py --stage")
    if not sys.flags.isolated or not sys.flags.no_site or not sys.flags.dont_write_bytecode:
        fail("start with Python -I -S -B")
    if sys.version_info[:3] != (3, 13, 5):
        fail("host Python version changed")
    verify_host_executable(sys.executable, EXPECTED_PYTHON, EXPECTED_PYTHON_SHA256, EXPECTED_PYTHON_SIZE)
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
    try:
        log_fd = open_stage_log(task_fd)
    finally:
        os.close(task_fd)
    log_stat = os.fstat(log_fd)
    if not stat.S_ISREG(log_stat.st_mode) or log_stat.st_nlink != 1 or (log_stat.st_uid, log_stat.st_gid) != (0, 0) or stat.S_IMODE(log_stat.st_mode) != 0o600:
        os.close(log_fd)
        fail("private stage log has an unsafe type, owner, or mode")
    null_fd = os.open("/dev/null", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    os.dup2(null_fd, 0)
    os.dup2(log_fd, 1)
    os.dup2(log_fd, 2)
    os.close(null_fd)
    os.close(log_fd)
    resource.setrlimit(resource.RLIMIT_CPU, (180, 180))
    resource.setrlimit(resource.RLIMIT_FSIZE, (32 << 20, 32 << 20))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_NPROC, (32, 32))
    resource.setrlimit(resource.RLIMIT_AS, (1 << 30, 1 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)


def read_private(task_fd, name, expected_sha256, limit, expected_mode=0o600):
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
    try:
        item = os.fstat(fd)
        if not stat.S_ISREG(item.st_mode) or item.st_nlink != 1 or (item.st_uid, item.st_gid) != (0, 0) or stat.S_IMODE(item.st_mode) != expected_mode:
            fail("task input has an unsafe type, owner, mode, or link count: " + name)
        if item.st_size > limit:
            fail("task input exceeds its size limit: " + name)
        data = bytearray()
        while len(data) <= limit:
            block = os.read(fd, min(1 << 20, limit + 1 - len(data)))
            if not block:
                break
            data.extend(block)
        if len(data) > limit or hash_bytes(data) != expected_sha256:
            fail("task input digest or size changed: " + name)
        return bytes(data)
    finally:
        os.close(fd)


def load_reviewed_tree_module(task_fd):
    source = read_private(task_fd, TREE_MODULE_NAME, EXPECTED_TREE_MODULE_SHA256, 1 << 20, 0o644)
    module = types.ModuleType("reviewed_rootfs_tree_functions")
    module.__file__ = TASK + "/" + TREE_MODULE_NAME
    exec(compile(source, module.__file__, "exec"), module.__dict__)
    return module


def open_abs_directory(path):
    if not path.startswith("/") or path == "/" or "." in path.split("/") or ".." in path.split("/"):
        fail("unsafe absolute directory path")
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        for component in path.split("/")[1:]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            opened = os.fstat(child)
            if not stat.S_ISDIR(opened.st_mode) or (opened.st_uid, opened.st_gid) != (0, 0):
                os.close(child)
                fail("source path has an unsafe directory component")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def source_parent(path):
    if not path.startswith("/") or "." in path.split("/") or ".." in path.split("/"):
        fail("unsafe source path")
    components = path.split("/")[1:]
    if not components or any(not part for part in components):
        fail("invalid source path")
    parent_path = "/" + "/".join(components[:-1]) if len(components) > 1 else "/"
    parent_fd = open_abs_directory(parent_path) if parent_path != "/" else os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    return parent_fd, components[-1]


def read_source_regular(entry):
    parent_fd, leaf = source_parent(entry["source"])
    try:
        fd = os.open(leaf, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
    finally:
        os.close(parent_fd)
    try:
        item = os.fstat(fd)
        expected = (entry["uid"], entry["gid"], entry["mode"], 1, entry["size"])
        actual = (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink, item.st_size)
        if not stat.S_ISREG(item.st_mode) or actual != expected or item.st_size > MAX_SINGLE_FILE_BYTES:
            fail("source file type, owner, mode, link count, or size changed: " + entry["source"])
        data = bytearray()
        while len(data) <= entry["size"]:
            block = os.read(fd, min(1 << 20, entry["size"] + 1 - len(data)))
            if not block:
                break
            data.extend(block)
        if len(data) != entry["size"] or hash_bytes(data) != entry["sha256"]:
            fail("source file digest changed: " + entry["source"])
        return bytes(data)
    finally:
        os.close(fd)


def verify_source_symlink(entry):
    parent_fd, leaf = source_parent(entry["source"])
    try:
        item = os.stat(leaf, dir_fd=parent_fd, follow_symlinks=False)
        if not stat.S_ISLNK(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink) != (entry["uid"], entry["gid"], entry["mode"], 1):
            fail("source symlink identity changed: " + entry["source"])
        if os.readlink(leaf, dir_fd=parent_fd) != entry["target"]:
            fail("source symlink target changed: " + entry["source"])
    finally:
        os.close(parent_fd)


def verify_source_dir(entry):
    fd = open_abs_directory(entry["path"])
    try:
        item = os.fstat(fd)
        if not stat.S_ISDIR(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode)) != (entry["uid"], entry["gid"], entry["mode"]):
            fail("source package directory identity changed: " + entry["path"])
    finally:
        os.close(fd)


def open_rootfs_parent(root_fd, path):
    if not path.startswith("/") or path == "/" or "." in path.split("/") or ".." in path.split("/"):
        fail("unsafe rootfs destination")
    components = path.split("/")[1:]
    parent = os.dup(root_fd)
    try:
        for component in components[:-1]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
            item = os.fstat(child)
            if not stat.S_ISDIR(item.st_mode) or (item.st_uid, item.st_gid) != (0, 0) or item.st_mode & 0o022:
                os.close(child)
                fail("unsafe rootfs destination parent")
            os.close(parent)
            parent = child
        return parent, components[-1]
    except BaseException:
        os.close(parent)
        raise


def verify_existing_dir(root_fd, entry):
    path = entry["path"]
    fd = open_abs_rootfs_dir(root_fd, path)
    try:
        item = os.fstat(fd)
        if (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode)) != (0, 0, entry["mode"]):
            fail("preexisting rootfs directory changed: " + path)
    finally:
        os.close(fd)


def open_abs_rootfs_dir(root_fd, path):
    if not path.startswith("/") or path == "/" or "." in path.split("/") or ".." in path.split("/"):
        fail("unsafe rootfs directory path")
    fd = os.dup(root_fd)
    try:
        for component in path.split("/")[1:]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            item = os.fstat(child)
            if not stat.S_ISDIR(item.st_mode) or (item.st_uid, item.st_gid) != (0, 0) or item.st_mode & 0o022:
                os.close(child)
                fail("unsafe rootfs directory component")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def check_inventory(inventory):
    if inventory.get("schema") != 1 or inventory.get("status") != "INVENTORY_ONLY; no rootfs writes; files are from installed Debian packages on bastion":
        fail("runtime inventory status or schema changed")
    expected_packages = {
        "libpython3.13-minimal:amd64": PYTHON_PACKAGE,
        "python3.13-minimal:amd64": INTERPRETER_PACKAGE,
        "python3-minimal": PYTHON_LINK_PACKAGE,
        "libexpat1:amd64": EXPAT_PACKAGE,
    }
    if inventory.get("packages") != expected_packages:
        fail("installed Debian package versions changed")
    contract = inventory.get("runtime_contract")
    if contract != {
        "interpreter": "/usr/bin/python3 -I -S -B",
        "helper_sha256": EXPECTED_HELPER_SHA256,
        "modules": ["hashlib", "json", "os", "pathlib", "stat", "sys"],
        "sha256_provider": "CPython built-in _sha2 fallback; _hashlib/openssl excluded",
        "site": "disabled by -S; sitecustomize omitted",
    }:
        fail("Python helper runtime contract changed")
    counts = inventory.get("counts")
    if counts != {
        "directories": EXPECTED_DIRECTORY_COUNT,
        "files": EXPECTED_FILE_COUNT,
        "regular_file_bytes": EXPECTED_REGULAR_BYTES,
        "symlinks": EXPECTED_SYMLINK_COUNT,
    }:
        fail("runtime inventory counts changed")
    if not isinstance(inventory.get("files"), list) or not isinstance(inventory.get("dirs"), list):
        fail("runtime inventory is malformed")
    regular_files = [entry for entry in inventory["files"] if entry.get("kind") == "file"]
    symlinks = [entry for entry in inventory["files"] if entry.get("kind") == "symlink"]
    if len(regular_files) != EXPECTED_FILE_COUNT or len(symlinks) != EXPECTED_SYMLINK_COUNT or len(inventory["dirs"]) != EXPECTED_DIRECTORY_COUNT:
        fail("runtime inventory entry count changed")
    if len(regular_files) + len(symlinks) != len(inventory["files"]):
        fail("runtime inventory contains an unsupported file type")
    destinations = [entry.get("destination") for entry in inventory["files"]]
    directory_paths = [entry.get("path") for entry in inventory["dirs"]]
    if len(set(destinations)) != len(destinations) or len(set(directory_paths)) != len(directory_paths):
        fail("runtime inventory has duplicate paths")
    if any(not path.startswith("/usr/lib/python3.13/") for path in destinations if path not in {
        "/usr/bin/python3", "/usr/bin/python3.13", "/lib/x86_64-linux-gnu/libexpat.so.1", "/lib/x86_64-linux-gnu/libexpat.so.1.12.3"
    }):
        fail("runtime inventory has an unexpected destination")
    if "/usr/lib/python3.13/lib-dynload/_hashlib.cpython-313-x86_64-linux-gnu.so" in destinations or "/usr/lib/python3.13/lib-dynload/_ssl.cpython-313-x86_64-linux-gnu.so" in destinations:
        fail("OpenSSL-dependent extension was included")
    if "/usr/lib/python3.13/sitecustomize.py" in destinations:
        fail("sitecustomize must remain outside the runtime image")
    if sum(entry.get("size", 0) for entry in inventory["files"] if entry.get("kind") == "file") != EXPECTED_REGULAR_BYTES:
        fail("runtime inventory byte count changed")
    if inventory.get("elf") != {
        "python3.13_needed": ["libm.so.6", "libz.so.1", "libexpat.so.1", "libc.so.6"],
        "python3.13_sha256": EXPECTED_PYTHON_SHA256,
        "expat_needed": ["libc.so.6"],
        "existing_rootfs_dependencies": ["libm.so.6", "libz.so.1", "libc.so.6", "ld-linux-x86-64.so.2"],
        "new_dependency": "libexpat.so.1 -> libexpat.so.1.12.3",
    }:
        fail("interpreter dynamic dependency inventory changed")


def create_dir(root_fd, path, mode):
    parent, leaf = open_rootfs_parent(root_fd, path)
    try:
        os.mkdir(leaf, mode, dir_fd=parent)
        child = os.open(leaf, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
        try:
            os.fchown(child, 0, 0)
            os.fchmod(child, mode)
            os.fsync(child)
            item = os.fstat(child)
            if not stat.S_ISDIR(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode)) != (0, 0, mode):
                fail("new rootfs directory identity changed: " + path)
        finally:
            os.close(child)
        os.fsync(parent)
    finally:
        os.close(parent)


def check_destination_absent(root_fd, path):
    if not path.startswith("/") or path == "/" or "." in path.split("/") or ".." in path.split("/"):
        fail("unsafe rootfs destination")
    components = path.split("/")[1:]
    parent = os.dup(root_fd)
    try:
        for component in components[:-1]:
            try:
                child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
            except FileNotFoundError:
                return
            item = os.fstat(child)
            if not stat.S_ISDIR(item.st_mode) or (item.st_uid, item.st_gid) != (0, 0) or item.st_mode & 0o022:
                os.close(child)
                fail("unsafe existing rootfs destination parent")
            os.close(parent)
            parent = child
        try:
            os.stat(components[-1], dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError:
            return
        fail("rootfs destination already exists: " + path)
    finally:
        os.close(parent)


def write_file(root_fd, entry, data):
    parent, leaf = open_rootfs_parent(root_fd, entry["destination"])
    try:
        fd = os.open(leaf, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, entry["mode"], dir_fd=parent)
        try:
            view = memoryview(data)
            while view:
                written = os.write(fd, view)
                if written <= 0:
                    fail("short rootfs file write: " + entry["destination"])
                view = view[written:]
            os.fchown(fd, 0, 0)
            os.fchmod(fd, entry["mode"])
            os.fsync(fd)
            item = os.fstat(fd)
            if not stat.S_ISREG(item.st_mode) or (item.st_uid, item.st_gid, stat.S_IMODE(item.st_mode), item.st_nlink, item.st_size) != (0, 0, entry["mode"], 1, entry["size"]):
                fail("staged rootfs file identity changed: " + entry["destination"])
        finally:
            os.close(fd)
        os.fsync(parent)
    finally:
        os.close(parent)


def create_symlink(root_fd, entry):
    parent, leaf = open_rootfs_parent(root_fd, entry["destination"])
    try:
        os.symlink(entry["target"], leaf, dir_fd=parent)
        item = os.stat(leaf, dir_fd=parent, follow_symlinks=False)
        if not stat.S_ISLNK(item.st_mode) or (item.st_uid, item.st_gid, os.readlink(leaf, dir_fd=parent)) != (0, 0, entry["target"]):
            fail("staged rootfs symlink identity changed: " + entry["destination"])
        os.fsync(parent)
    finally:
        os.close(parent)


def main():
    start_boundary()
    task_fd = open_task_dir()
    try:
        inventory_bytes = read_private(task_fd, INVENTORY_NAME, EXPECTED_INVENTORY_SHA256, MAX_INVENTORY_BYTES)
        inventory = json.loads(inventory_bytes)
        check_inventory(inventory)
        helper = read_private(task_fd, HELPER_NAME, EXPECTED_HELPER_SHA256, 1 << 20, 0o444)
        if len(helper) != 15684:
            fail("reviewed managed-home helper size changed")
        parent_manifest = read_private(task_fd, PARENT_MANIFEST_NAME, EXPECTED_PARENT_MANIFEST_SHA256, 8192)
        input_manifest = read_private(task_fd, INPUT_MANIFEST_NAME, EXPECTED_INPUT_MANIFEST_SHA256, 8192)
        input_result = read_private(task_fd, INPUT_RESULT_NAME, EXPECTED_INPUT_RESULT_SHA256, 8192)
        parent_doc = json.loads(parent_manifest)
        input_doc = json.loads(input_manifest)
        input_result_doc = json.loads(input_result)
        if parent_doc.get("result_rootfs_tree_sha256") != EXPECTED_PARENT_TREE_SHA256:
            fail("Rustup parent delta result changed")
        if input_doc.get("result_rootfs_tree_sha256") != EXPECTED_INPUT_TREE_SHA256 or input_result_doc.get("result_tree_sha256") != EXPECTED_INPUT_TREE_SHA256:
            fail("empty input mountpoint delta result changed")
        if parent_doc.get("parent_rootfs_tree_sha256") != EXPECTED_INPUT_TREE_SHA256 or parent_doc.get("asset_sha256") != "20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c":
            fail("Rustup parent provenance chain changed")
        tree = load_reviewed_tree_module(task_fd)
        root_fd = os.open(ROOTFS_NAME, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
        try:
            before = tree.tree_records(root_fd)
            if tree.digest_records(before) != EXPECTED_PARENT_TREE_SHA256:
                fail("rootfs does not match the sealed Rustup parent tree")
            before_by_path = {record["path"]: record for record in before}
            if len(before_by_path) != len(before):
                fail("parent rootfs has duplicate paths")

            dirs = inventory["dirs"]
            files = inventory["files"]
            new_dirs = [entry for entry in dirs if entry["source"] != "preexisting-parent"]
            existing_dirs = [entry for entry in dirs if entry["source"] == "preexisting-parent"]
            if len(new_dirs) + len(existing_dirs) != EXPECTED_DIRECTORY_COUNT:
                fail("directory plan is inconsistent")
            for entry in dirs:
                if entry["source"] == "package-directory":
                    verify_source_dir(entry)
            for entry in existing_dirs:
                verify_existing_dir(root_fd, entry)
            for entry in new_dirs:
                check_destination_absent(root_fd, entry["path"])
            for entry in files:
                check_destination_absent(root_fd, entry["destination"])

            payloads = {}
            for entry in files:
                if entry["kind"] == "file":
                    payloads[entry["destination"]] = read_source_regular(entry)
                elif entry["kind"] == "symlink":
                    verify_source_symlink(entry)
                else:
                    fail("unsupported runtime inventory entry type")

            for entry in new_dirs:
                create_dir(root_fd, entry["path"], entry["mode"])
                print("created-directory=" + entry["path"], flush=True)
            for entry in files:
                if entry["kind"] == "file":
                    write_file(root_fd, entry, payloads[entry["destination"]])
                else:
                    create_symlink(root_fd, entry)
                print("staged-" + entry["kind"] + "=" + entry["destination"], flush=True)

            after = tree.tree_records(root_fd)
            after_by_path = {record["path"]: record for record in after}
            expected = dict(before_by_path)
            for entry in new_dirs:
                expected[entry["path"]] = {"path": entry["path"], "type": "directory", "mode": entry["mode"], "uid": 0, "gid": 0}
            for entry in files:
                if entry["kind"] == "file":
                    expected[entry["destination"]] = {
                        "path": entry["destination"], "type": "file", "mode": entry["mode"], "uid": 0, "gid": 0,
                        "size": entry["size"], "sha256": entry["sha256"],
                    }
                else:
                    expected[entry["destination"]] = {
                        "path": entry["destination"], "type": "symlink", "mode": 0o777, "uid": 0, "gid": 0,
                        "target": entry["target"],
                    }
            if after_by_path != expected or len(after) != len(expected):
                fail("rootfs changed beyond the exact Python runtime addition")
            result_tree = tree.digest_records(after)
        finally:
            os.close(root_fd)

        additions = [record for record in after if record["path"] not in before_by_path]
        document = {
            "schema": 1,
            "status": "PYTHON_3_13_MINIMAL_RUNTIME_STAGED; NOT_EXECUTED; Cargo and generator remain pending",
            "parent_rootfs_tree_sha256": EXPECTED_PARENT_TREE_SHA256,
            "parent_rustup_delta_manifest_sha256": EXPECTED_PARENT_MANIFEST_SHA256,
            "parent_input_delta_manifest_sha256": EXPECTED_INPUT_MANIFEST_SHA256,
            "parent_input_verification_result_sha256": EXPECTED_INPUT_RESULT_SHA256,
            "inventory_sha256": EXPECTED_INVENTORY_SHA256,
            "runtime_contract": inventory["runtime_contract"],
            "debian_packages": inventory["packages"],
            "added_paths": additions,
            "added_file_bytes": EXPECTED_REGULAR_BYTES,
            "excluded": inventory["excluded"],
            "elf_dependency_closure": inventory["elf"],
            "result_rootfs_tree_sha256": result_tree,
            "execution": "The host Python staged these exact bytes. The staged interpreter, helper, Rustup, Mise, Cargo, compiler, network, container, and account were not run.",
            "reversal": "Remove only the listed files and symlinks after verifying each recorded identity; remove only listed directories after verifying they are empty.",
            "timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        }
        manifest_bytes = (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
        manifest_fd = os.open(MANIFEST_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)
        try:
            view = memoryview(manifest_bytes)
            while view:
                written = os.write(manifest_fd, view)
                if written <= 0:
                    fail("short Python runtime delta manifest write")
                view = view[written:]
            os.fsync(manifest_fd)
        finally:
            os.close(manifest_fd)
        os.fsync(task_fd)
        print("PASS: staged pinned Python runtime without executing it; tree_sha256=" + result_tree + " manifest_sha256=" + hash_bytes(manifest_bytes), flush=True)
    finally:
        os.close(task_fd)


if __name__ == "__main__":
    main()

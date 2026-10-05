#!/usr/bin/python3
"""Add only the reviewed empty /input mountpoint to the sealed base rootfs."""
import ctypes
import datetime
import hashlib
import json
import os
import posixpath
import resource
import stat
import sys

TASK = "/root/.velnor-generator-preview-20261005"
ROOTFS_NAME = "rootfs"
BASE_MANIFEST_NAME = "rootfs-base-manifest.json"
OLD_PLAN_NAME = "rootfs-stage-plan.json"
NEW_PLAN_NAME = "rootfs-stage-plan-with-input.json"
PROVENANCE_NAME = "official-tool-asset-provenance.json"
DELTA_MANIFEST_NAME = "rootfs-input-delta-manifest.json"
LOG_NAME = "rootfs-input-delta.log"
EXPECTED_BASE_MANIFEST = "1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf"
EXPECTED_OLD_PLAN = "ebca0ef5f4c716e533cece9637b3d68763cc8265ce4016bf84a85a279416e77f"
EXPECTED_NEW_PLAN = "4c4a18dd85d71aaa9c9fc9352da9553fc2eb7f5c2d10a8365058c19ea04597bc"
EXPECTED_PROVENANCE = "67a8ba245858b11276a6496a513584fefec26e26218b0cdfad028a3fac07d137"
EXPECTED_BASE_TREE = "1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763"
EXPECTED_INPUT_RECORD = {"path": "/input", "type": "directory", "mode": 493, "uid": 0, "gid": 0}


def fail(message):
    raise SystemExit("rootfs input delta: " + message)


def hash_bytes(data):
    return hashlib.sha256(data).hexdigest()


def open_task_dir():
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        for index, part in enumerate(TASK.split("/")[1:]):
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            st = os.fstat(child)
            final = index == len(TASK.split("/")[1:]) - 1
            if not stat.S_ISDIR(st.st_mode) or (st.st_uid, st.st_gid) != (0, 0):
                os.close(child)
                fail("task path contains a non-root directory")
            if final:
                if stat.S_IMODE(st.st_mode) != 0o700:
                    os.close(child)
                    fail("task directory mode changed")
            elif st.st_mode & 0o022:
                os.close(child)
                fail("task ancestor is group/world writable")
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def start_boundary():
    if sys.argv[1:] != ["--add-input"]:
        fail("usage: rootfs-input-delta.py --add-input")
    if sys.flags.isolated != 1 or sys.flags.no_site != 1:
        fail("start with Python -I -S")
    if os.environ != {"PATH": "/usr/bin:/bin", "LC_ALL": "C"}:
        fail("run only under the exact clean environment")
    if os.geteuid() != 0 or os.getegid() != 0 or any(group != 0 for group in os.getgroups()):
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
        fail("could not close inherited file descriptors")
    task_fd = open_task_dir()
    log_fd = os.open(LOG_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)
    st = os.fstat(log_fd)
    if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600:
        fail("private log has an unsafe type, owner, or mode")
    null_fd = os.open("/dev/null", os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
    os.dup2(null_fd, 0)
    os.dup2(log_fd, 1)
    os.dup2(log_fd, 2)
    os.close(null_fd)
    os.close(log_fd)
    os.close(task_fd)
    resource.setrlimit(resource.RLIMIT_CPU, (60, 60))
    resource.setrlimit(resource.RLIMIT_FSIZE, (1 << 20, 1 << 20))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_NPROC, (32, 32))
    resource.setrlimit(resource.RLIMIT_AS, (1 << 30, 1 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    os.umask(0o077)


def read_task_file(task_fd, name, expected_sha, limit):
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or (st.st_uid, st.st_gid) != (0, 0) or stat.S_IMODE(st.st_mode) != 0o600:
            fail("task input has an unsafe type, owner, or mode: " + name)
        if st.st_size > limit:
            fail("task input exceeds its size limit: " + name)
        data = bytearray()
        while len(data) <= limit:
            chunk = os.read(fd, min(1 << 20, limit + 1 - len(data)))
            if not chunk:
                break
            data.extend(chunk)
        if len(data) > limit or hash_bytes(data) != expected_sha:
            fail("task input digest or size changed: " + name)
        return bytes(data)
    finally:
        os.close(fd)


def tree_records(root_fd):
    records = [{"path": "/", "type": "directory", "mode": 0o755, "uid": 0, "gid": 0}]

    def walk(fd, prefix):
        for name in sorted(os.listdir(fd), key=os.fsencode):
            if name in (".", "..") or "/" in name:
                fail("invalid rootfs directory entry")
            path = (prefix + "/" + name) if prefix else "/" + name
            st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            mode = stat.S_IMODE(st.st_mode)
            if (st.st_uid, st.st_gid) != (0, 0):
                fail("rootfs owner mismatch")
            if stat.S_ISCHR(st.st_mode) and path == "/dev/null" and os.major(st.st_rdev) == 1 and os.minor(st.st_rdev) == 3:
                if mode != 0o666:
                    fail("unexpected null device mode")
                records.append({"path": path, "type": "character-device", "mode": mode, "uid": 0, "gid": 0, "major": 1, "minor": 3})
                continue
            if stat.S_ISLNK(st.st_mode):
                target = os.readlink(name, dir_fd=fd)
                normalized = posixpath.normpath(target if target.startswith("/") else posixpath.join(posixpath.dirname(path), target))
                if not normalized.startswith("/") or normalized == "/.." or normalized.startswith("/../"):
                    fail("rootfs symlink escape")
                records.append({"path": path, "type": "symlink", "mode": mode, "uid": 0, "gid": 0, "target": target})
                continue
            if mode & 0o022:
                fail("rootfs writable-by-group-or-world path")
            if stat.S_ISDIR(st.st_mode):
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                try:
                    opened = os.fstat(child)
                    if not stat.S_ISDIR(opened.st_mode) or (opened.st_dev, opened.st_ino) != (st.st_dev, st.st_ino):
                        fail("rootfs directory changed during hashing")
                    records.append({"path": path, "type": "directory", "mode": mode, "uid": 0, "gid": 0})
                    walk(child, path)
                finally:
                    os.close(child)
            elif stat.S_ISREG(st.st_mode):
                if st.st_nlink != 1:
                    fail("rootfs hard link rejected")
                child = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                try:
                    opened = os.fstat(child)
                    if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (st.st_dev, st.st_ino):
                        fail("rootfs file changed during hashing")
                    digest = hashlib.sha256()
                    size = 0
                    while True:
                        block = os.read(child, 1 << 20)
                        if not block:
                            break
                        digest.update(block)
                        size += len(block)
                    if size != st.st_size:
                        fail("rootfs file changed size during hashing")
                    records.append({"path": path, "type": "file", "mode": mode, "uid": 0, "gid": 0, "size": size, "sha256": digest.hexdigest()})
                finally:
                    os.close(child)
            else:
                fail("unsupported rootfs entry")

    root_st = os.fstat(root_fd)
    if not stat.S_ISDIR(root_st.st_mode) or (root_st.st_uid, root_st.st_gid) != (0, 0) or stat.S_IMODE(root_st.st_mode) != 0o755:
        fail("rootfs root mode mismatch")
    walk(root_fd, "")
    return records


def digest_records(records):
    encoded = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
    return hash_bytes(encoded)


def main():
    start_boundary()
    task_fd = open_task_dir()
    try:
        base_manifest_bytes = read_task_file(task_fd, BASE_MANIFEST_NAME, EXPECTED_BASE_MANIFEST, 4096)
        old_plan_bytes = read_task_file(task_fd, OLD_PLAN_NAME, EXPECTED_OLD_PLAN, 1 << 20)
        new_plan_bytes = read_task_file(task_fd, NEW_PLAN_NAME, EXPECTED_NEW_PLAN, 1 << 20)
        provenance_bytes = read_task_file(task_fd, PROVENANCE_NAME, EXPECTED_PROVENANCE, 1 << 20)
        base_manifest = json.loads(base_manifest_bytes)
        old_plan = json.loads(old_plan_bytes)
        new_plan = json.loads(new_plan_bytes)
        provenance = json.loads(provenance_bytes)
        if provenance.get("schema") != 1:
            fail("tool provenance schema changed")
        if base_manifest != {
            "base_rootfs_tree_sha256": EXPECTED_BASE_TREE,
            "materializer_sha256": "4aba299ea3cb925bae7413bd6b8b45845d1ce83a1f617d964b9068dbf87da0a3",
            "plan_sha256": EXPECTED_OLD_PLAN,
            "schema": 1,
            "status": "BASE_MATERIALIZED; cargo seed and final stage manifest are still absent",
            "tool_provenance_sha256": EXPECTED_PROVENANCE,
        }:
            fail("sealed base manifest fields changed")
        if old_plan.get("tool_provenance_sha256") != EXPECTED_PROVENANCE or new_plan.get("tool_provenance_sha256") != EXPECTED_PROVENANCE:
            fail("plan provenance changed")
        old_dirs = old_plan.get("rootfs_dirs")
        new_dirs = new_plan.get("rootfs_dirs")
        expected = {"mode": "0755", "owner": "root:root", "path": "/input"}
        if not isinstance(old_dirs, list) or not isinstance(new_dirs, list) or expected in old_dirs or new_dirs.count(expected) != 1:
            fail("input mountpoint directory declaration is invalid")
        old_without_input = dict(new_plan)
        old_without_input["rootfs_dirs"] = [item for item in new_dirs if item != expected]
        if old_without_input != old_plan:
            fail("new plan differs beyond the single /input directory")
        if hash_bytes((json.dumps(new_plan, sort_keys=True, separators=(",", ":")) + "\n").encode()) != EXPECTED_NEW_PLAN:
            fail("new plan canonical digest changed")
        try:
            os.stat(DELTA_MANIFEST_NAME, dir_fd=task_fd, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            fail("delta manifest already exists")

        root_fd = os.open(ROOTFS_NAME, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=task_fd)
        try:
            before_records = tree_records(root_fd)
            before_digest = digest_records(before_records)
            if before_digest != EXPECTED_BASE_TREE:
                fail("sealed base rootfs tree changed")
            try:
                os.stat("input", dir_fd=root_fd, follow_symlinks=False)
            except FileNotFoundError:
                pass
            else:
                fail("/input already exists")
            os.mkdir("input", 0o755, dir_fd=root_fd)
            input_fd = os.open("input", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=root_fd)
            try:
                input_st = os.fstat(input_fd)
                if not stat.S_ISDIR(input_st.st_mode) or (input_st.st_uid, input_st.st_gid) != (0, 0):
                    fail("created /input identity changed")
                if stat.S_IMODE(input_st.st_mode) != 0o755:
                    os.fchmod(input_fd, 0o755)
                    input_st = os.fstat(input_fd)
                if stat.S_IMODE(input_st.st_mode) != 0o755 or os.listdir(input_fd):
                    fail("created /input is not an empty mode0755 directory")
            finally:
                os.close(input_fd)
            after_records = tree_records(root_fd)
            if [record for record in after_records if record["path"] != "/input"] != before_records:
                fail("rootfs changed beyond the requested /input directory")
            added = [record for record in after_records if record["path"] == "/input"]
            if added != [EXPECTED_INPUT_RECORD]:
                fail("added rootfs record differs from the /input contract")
            after_digest = digest_records(after_records)
        finally:
            os.close(root_fd)

        manifest = {
            "schema": 1,
            "status": "BASE_ROOTFS_PLUS_EMPTY_INPUT_MOUNTPOINT; cargo seed absent",
            "base_manifest_sha256": EXPECTED_BASE_MANIFEST,
            "base_rootfs_tree_sha256": before_digest,
            "plan_sha256": EXPECTED_NEW_PLAN,
            "tool_provenance_sha256": EXPECTED_PROVENANCE,
            "added_path": "/input",
            "added_record": EXPECTED_INPUT_RECORD,
            "result_rootfs_tree_sha256": after_digest,
            "reversal": "Remove only empty rootfs/input after verifying this delta manifest and directory identity.",
        }
        data = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode()
        manifest_fd = os.open(DELTA_MANIFEST_NAME, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=task_fd)
        try:
            view = memoryview(data)
            while view:
                written = os.write(manifest_fd, view)
                if written <= 0:
                    fail("short write for input delta manifest")
                view = view[written:]
            os.fchmod(manifest_fd, 0o600)
        finally:
            os.close(manifest_fd)
        print("PASS: added exactly empty /input; base_tree=" + before_digest + " result_tree=" + after_digest + " delta_manifest_sha256=" + hash_bytes(data))
    finally:
        os.close(task_fd)


if __name__ == "__main__":
    main()

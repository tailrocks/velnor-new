#!/usr/bin/python3
"""Hash-bound private host bootstrap for the offline Velnor generator runner."""
import hashlib
import json
import os
import posixpath
import resource
import stat
import sys
import ctypes

TASK = "/root/.velnor-generator-preview-20261005"
RUNNER = TASK + "/preseed-preview-runner.sh"
NAMESPACE = TASK + "/preseed-preview-namespace.sh"
INSIDE = TASK + "/preseed-preview-inside.sh"
STAGE_MANIFEST = TASK + "/rootfs-stage-manifest.json"
BASE_MANIFEST = TASK + "/rootfs-base-manifest.json"
ROOTFS = TASK + "/rootfs"
TOOL_PROVENANCE = TASK + "/official-tool-asset-provenance.json"
PLAN = TASK + "/rootfs-stage-plan.json"
LOCK_INVENTORY = TASK + "/lock-source-inventory.json"
RUNNER_SHA256 = None
NAMESPACE_SHA256 = None
INSIDE_SHA256 = None
PLAN_SHA256 = "ebca0ef5f4c716e533cece9637b3d68763cc8265ce4016bf84a85a279416e77f"
BASE_MANIFEST_SHA256 = "1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf"
STAGE_MANIFEST_SHA256 = None
ROOTFS_TREE_SHA256 = "1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763"
MATERIALIZER_SHA256 = "4aba299ea3cb925bae7413bd6b8b45845d1ce83a1f617d964b9068dbf87da0a3"
CACHE_SEEDER_SHA256 = None
TOOL_PROVENANCE_SHA256 = "67a8ba245858b11276a6496a513584fefec26e26218b0cdfad028a3fac07d137"
LOCK_INVENTORY_SHA256 = "86151c2aaaa89e8eb579ec3a533b0f3fbb2d3344d272528b0fe46ad4c6f92b1f"
CARGO_LOCK_SHA256 = "b43ddef9c7a6b7e4c33cb6a8a4dd3659f26a7645a0f0c09782e04b2ee5b898a4"


def fail(message):
    os.write(2, ("preseed bootstrap: " + message + "\n").encode())
    raise SystemExit(1)


def check_mode(st, expected_uid=0, expected_gid=0, exact_mode=None):
    if st.st_uid != expected_uid or st.st_gid != expected_gid:
        fail("unexpected owner")
    mode = stat.S_IMODE(st.st_mode)
    if exact_mode is not None and mode != exact_mode:
        fail("unexpected mode")
    if mode & 0o022:
        fail("group or world writable path")


def open_dir_chain(path, final_uid=0, final_gid=0, final_mode=None):
    if not path.startswith("/"):
        fail("expected absolute directory")
    fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for index, part in enumerate(path.split("/")[1:]):
            if not part or part in (".", ".."):
                fail("invalid path component")
            child = os.open(
                part,
                os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                dir_fd=fd,
            )
            st = os.fstat(child)
            final = index == len(path.split("/")[1:]) - 1
            if final:
                check_mode(st, final_uid, final_gid, final_mode)
            else:
                check_mode(st, 0, 0)
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def read_regular(path, expected_sha=None, expected_uid=0, expected_gid=0, expected_mode=None, limit=1 << 28):
    parent, name = posixpath.split(path)
    pfd = open_dir_chain(parent)
    try:
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=pfd)
    finally:
        os.close(pfd)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or st.st_size > limit:
            fail("unsafe regular file")
        check_mode(st, expected_uid, expected_gid, expected_mode)
        digest = hashlib.sha256()
        chunks = []
        while True:
            chunk = os.read(fd, 1 << 20)
            if not chunk:
                break
            digest.update(chunk)
            chunks.append(chunk)
        got = digest.hexdigest()
        if expected_sha is not None and got != expected_sha:
            fail("file digest mismatch")
        return b"".join(chunks), got
    finally:
        os.close(fd)


def safe_symlink_target(path, target):
    if not isinstance(target, str) or not target:
        fail("empty symlink target")
    parent = posixpath.dirname(path)
    normalized = posixpath.normpath(target if target.startswith("/") else posixpath.join(parent, target))
    if normalized == "/.." or normalized.startswith("/../") or not normalized.startswith("/"):
        fail("symlink escapes rootfs")


def tree_hash(root):
    rootfd = open_dir_chain(root, final_mode=0o755)
    records = []

    def walk(fd, prefix):
        for name in sorted(os.listdir(fd), key=os.fsencode):
            if name in (".", "..") or "/" in name:
                fail("invalid directory entry")
            path = (prefix + "/" + name) if prefix else "/" + name
            st = os.stat(name, dir_fd=fd, follow_symlinks=False)
            mode = stat.S_IMODE(st.st_mode)
            if st.st_uid != 0 or st.st_gid != 0:
                fail("untrusted rootfs owner")
            if stat.S_ISCHR(st.st_mode) and path == "/dev/null" and os.major(st.st_rdev) == 1 and os.minor(st.st_rdev) == 3:
                if mode != 0o666:
                    fail("unexpected null device mode")
                records.append({"path": path, "type": "character-device", "mode": mode, "uid": 0, "gid": 0, "major": 1, "minor": 3})
                continue
            if stat.S_ISLNK(st.st_mode):
                target = os.readlink(name, dir_fd=fd)
                safe_symlink_target(path, target)
                records.append({"path": path, "type": "symlink", "mode": mode, "uid": 0, "gid": 0, "target": target})
                continue
            if mode & 0o022:
                fail("writable-by-group-or-world rootfs path")
            if stat.S_ISDIR(st.st_mode):
                child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                try:
                    if os.fstat(child).st_ino != st.st_ino:
                        fail("rootfs directory changed during verification")
                    records.append({"path": path, "type": "directory", "mode": mode, "uid": st.st_uid, "gid": st.st_gid})
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
                        fail("rootfs file changed during verification")
                    digest = hashlib.sha256()
                    while True:
                        chunk = os.read(child, 1 << 20)
                        if not chunk:
                            break
                        digest.update(chunk)
                    records.append({"path": path, "type": "file", "mode": mode, "uid": st.st_uid, "gid": st.st_gid, "size": st.st_size, "sha256": digest.hexdigest()})
                finally:
                    os.close(child)
            else:
                fail("unsupported rootfs object type")

    try:
        rootst = os.fstat(rootfd)
        records.append({"path": "/", "type": "directory", "mode": stat.S_IMODE(rootst.st_mode), "uid": rootst.st_uid, "gid": rootst.st_gid})
        walk(rootfd, "")
    finally:
        os.close(rootfd)
    payload = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
    return hashlib.sha256(payload).hexdigest()


def verify_host_tools(plan_bytes):
    plan = json.loads(plan_bytes)
    tools = plan.get("host_launcher_files")
    if not isinstance(tools, list) or not tools:
        fail("host launcher inventory missing")
    resolved = set()
    for tool in tools:
        path = tool.get("path")
        target = tool.get("resolved_path")
        digest = tool.get("sha256")
        size = tool.get("size")
        if not all(isinstance(value, str) and value.startswith("/") for value in (path, target)):
            fail("invalid host launcher path")
        if not isinstance(size, int) or size < 1 or not isinstance(digest, str) or len(digest) != 64:
            fail("invalid host launcher digest record")
        if os.path.realpath(path) != target:
            fail("host launcher path resolution changed")
        if target not in resolved:
            data, _ = read_regular(target, digest, limit=size + 1)
            if len(data) != size:
                fail("host launcher size changed")
            st = os.stat(target, follow_symlinks=False)
            if not stat.S_IMODE(st.st_mode) & 0o111:
                fail("host launcher is not executable")
            resolved.add(target)
    python_record = next((item for item in tools if item.get("path") == "/usr/bin/python3"), None)
    if python_record is None or os.path.realpath(sys.executable) != python_record.get("resolved_path"):
        fail("bootstrap Python path differs from the pinned host runtime")


def check_profile_paths(profile):
    if profile == "main":
        source = TASK + "/main-source-full"
        archive = TASK + "/main-source-ecf-tree.tar"
        helper = TASK + "/main-helper-37279676047/velnor-actions"
        preseed = TASK + "/main-helper-37279676047/preseed-manifest.json"
        archive_sha = "c2b3c12edfc19e1553e8cfcf5f24f06844191a4a4cce45dc44f45a8eef20d28a"
        helper_sha = "048b4322ed3875941fc72cc1f5bae91c4d0b89fab203772663ff18fbe008c53c"
        preseed_sha = "33fd434cf0fb99a74798946c89bcd8f2a306d2508a09efe056b4f1f16ee19049"
        commit = "ecf54b771c0db11e03e54fa2c98cf531e5912585"
        tree = "0e4e8eac8d9ea32df264d74e614427936480af7d"
    else:
        source = TASK + "/pr65-source"
        archive = TASK + "/pr65-source-tree.tar"
        helper = TASK + "/pr65-helper-37275543333/velnor-actions"
        preseed = TASK + "/pr65-helper-37275543333/preseed-manifest.json"
        archive_sha = "c3a6a18ec1c80ab9e50db562df2f28fb26325a7de97ea2f95893ff77446be56d"
        helper_sha = "58b7d4c9473cdac6381b21e83f7508547e1d52c2f6f5962c4c22cf200316ba93"
        preseed_sha = "4db220b92aafdbdb1ec949d84b53e9b6060a2a96d5a884750da8b936c6c37abf"
        commit = "ff41745394712604bddf94b6a4e9f1069100789e"
        tree = "1546bdd222f10b429a23769e69c02ac28610768f"

    source_fd = open_dir_chain(source, final_mode=0o755)
    os.close(source_fd)
    _, archive_digest = read_regular(archive, archive_sha, expected_mode=0o644, limit=32 << 20)
    _, helper_digest = read_regular(helper, helper_sha, expected_mode=0o755, limit=32 << 20)
    preseed_bytes, _ = read_regular(preseed, preseed_sha, expected_mode=0o600, limit=4096)
    preseed_manifest = json.loads(preseed_bytes)
    if preseed_manifest != {
        "schema": 1,
        "commit": ("eec43f9c4c5f6113e4d184f9696a7a1c7c2ec6e2" if profile == "pr65" else commit),
        "target": "x86_64-unknown-linux-gnu",
        "toolchain": "rust@1.98.1+mr-boxington@1.21.1",
        "sha256": helper_sha,
    }:
        fail("preseed manifest fields differ from the immutable pair")
    return source, commit, tree, archive_digest, helper_digest


def main():
    if os.environ != {"PATH": "/usr/bin:/bin"}:
        fail("bootstrap requires exact env -i PATH")
    if sys.argv[1:] not in (["main"], ["pr65"]):
        fail("usage: preseed-preview-bootstrap.py {main|pr65}")
    profile = sys.argv[1]
    if os.geteuid() != 0 or os.getegid() != 0:
        fail("bootstrap requires effective root for the reviewed namespace setup")
    if os.uname().nodename.split(".", 1)[0] != "bastion":
        fail("host identity mismatch")

    taskfd = open_dir_chain(TASK, final_mode=0o700)
    logfd = -1
    logfile = -1
    try:
        try:
            os.mkdir("logs", 0o700, dir_fd=taskfd)
        except FileExistsError:
            pass
        logfd = os.open("logs", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=taskfd)
        logst = os.fstat(logfd)
        check_mode(logst, 0, 0, 0o700)
        logname = "runner-" + profile + ".log"
        logfile = os.open(logname, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=logfd)
        logst = os.fstat(logfile)
        if not stat.S_ISREG(logst.st_mode) or logst.st_nlink != 1:
            fail("unsafe private result log")
        check_mode(logst, 0, 0, 0o600)
        os.dup2(os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC), 0)
        os.dup2(logfile, 1)
        os.dup2(logfile, 2)
    finally:
        if logfile >= 0:
            os.close(logfile)
        if logfd >= 0:
            os.close(logfd)
        os.close(taskfd)

    close_range = getattr(ctypes.CDLL(None, use_errno=True), "close_range", None)
    if close_range is None or close_range(3, 0xFFFFFFFF, 0) != 0:
        fail("could not close inherited file descriptors")

    if any(value is None for value in (RUNNER_SHA256, NAMESPACE_SHA256, INSIDE_SHA256, STAGE_MANIFEST_SHA256, CACHE_SEEDER_SHA256)):
        fail("final runner, Cargo source seed, or stage manifest has no immutable seal")
    read_regular(RUNNER, RUNNER_SHA256, expected_mode=0o700, limit=32768)
    read_regular(NAMESPACE, NAMESPACE_SHA256, expected_mode=0o700, limit=32768)
    read_regular(INSIDE, INSIDE_SHA256, expected_mode=0o700, limit=32768)
    read_regular(TOOL_PROVENANCE, TOOL_PROVENANCE_SHA256, expected_mode=0o600, limit=1 << 20)
    read_regular(LOCK_INVENTORY, LOCK_INVENTORY_SHA256, expected_mode=0o600, limit=1 << 20)
    plan_bytes, _ = read_regular(PLAN, PLAN_SHA256, expected_mode=0o600, limit=1 << 20)
    verify_host_tools(plan_bytes)
    read_regular(TASK + "/preseed-preview-materialize.py", MATERIALIZER_SHA256, expected_mode=0o644, limit=1 << 20)
    base_manifest_bytes, _ = read_regular(BASE_MANIFEST, BASE_MANIFEST_SHA256, expected_mode=0o600, limit=1 << 20)
    base_manifest = json.loads(base_manifest_bytes)
    if base_manifest != {
        "base_rootfs_tree_sha256": ROOTFS_TREE_SHA256,
        "materializer_sha256": MATERIALIZER_SHA256,
        "plan_sha256": PLAN_SHA256,
        "schema": 1,
        "status": "BASE_MATERIALIZED; cargo seed and final stage manifest are still absent",
        "tool_provenance_sha256": TOOL_PROVENANCE_SHA256,
    }:
        fail("base rootfs manifest does not match the executed materialization")
    manifest_bytes, manifest_sha = read_regular(STAGE_MANIFEST, STAGE_MANIFEST_SHA256, expected_mode=0o600, limit=1 << 20)
    manifest = json.loads(manifest_bytes)
    if manifest.get("schema") != 1 or manifest.get("rootfs_tree_sha256") != ROOTFS_TREE_SHA256:
        fail("stage manifest schema or rootfs binding mismatch")
    if manifest.get("materializer_sha256") != MATERIALIZER_SHA256 or manifest.get("tool_provenance_sha256") != TOOL_PROVENANCE_SHA256:
        fail("stage manifest materializer/tool provenance mismatch")
    if manifest.get("cargo_lock_sha256") != CARGO_LOCK_SHA256 or manifest.get("cargo_registry_packages") != 74:
        fail("stage manifest cache lock binding mismatch")
    actual_tree = tree_hash(ROOTFS)
    if actual_tree != ROOTFS_TREE_SHA256:
        fail("rootfs tree digest mismatch")
    source, commit, tree, archive_digest, helper_digest = check_profile_paths(profile)
    work = TASK + "/work/" + profile
    output = TASK + "/out/" + profile
    workfd = open_dir_chain(work, final_uid=65534, final_gid=65534, final_mode=0o700)
    outfd = open_dir_chain(output, final_uid=65534, final_gid=65534, final_mode=0o700)
    try:
        if os.listdir(workfd) or os.listdir(outfd):
            fail("task work/output directory is not empty")
    finally:
        os.close(workfd)
        os.close(outfd)

    os.chdir("/")
    resource.setrlimit(resource.RLIMIT_CPU, (600, 600))
    resource.setrlimit(resource.RLIMIT_FSIZE, (1 << 30, 1 << 30))
    resource.setrlimit(resource.RLIMIT_NPROC, (128, 128))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_AS, (8 << 30, 8 << 30))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    env = {
        "PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "LANG": "C", "LC_ALL": "C",
        "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0",
    }
    os.execve("/usr/bin/bash", ["/usr/bin/bash", "--noprofile", "--norc", RUNNER, profile], env)


if __name__ == "__main__":
    main()

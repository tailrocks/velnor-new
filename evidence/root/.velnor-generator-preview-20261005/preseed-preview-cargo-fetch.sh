#!/usr/bin/bash
# Fetch only the exact lockfile's public crates.io archives in the private task rootfs.
set -Eeuo pipefail
umask 077

readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly ROOTFS="$TASK_DIR/rootfs"
readonly SOURCE_DIR=/tmp/velnor-g0-provenance-20261005
readonly WORK_DIR="$TASK_DIR/cargo-fetch-bc7-work"
readonly LOG_DIR="$TASK_DIR/logs"
readonly LOG_FILE="$LOG_DIR/cargo-fetch-bc7.log"
readonly MANIFEST_FILE="$TASK_DIR/cargo-source-seed-bc7-manifest.json"
readonly BASE_MANIFEST="$TASK_DIR/rootfs-base-manifest.json"
readonly EXPECTED_BASE_MANIFEST_SHA256=1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf
readonly EXPECTED_BASE_ROOTFS_TREE_SHA256=1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763
readonly EXPECTED_PLAN_SHA256=ebca0ef5f4c716e533cece9637b3d68763cc8265ce4016bf84a85a279416e77f
readonly EXPECTED_TOOL_PROVENANCE_SHA256=67a8ba245858b11276a6496a513584fefec26e26218b0cdfad028a3fac07d137
readonly NAMESPACE_SCRIPT="$TASK_DIR/preseed-preview-cargo-fetch-namespace.sh"
readonly EXPECTED_SOURCE_COMMIT=bc7f784a51233bd29676353392c9fc07c0ad01a7
readonly EXPECTED_SOURCE_TREE=b3939a4d6010f243485fb528f73af74d49b2fb94
readonly EXPECTED_MAIN_PARENT=703d1b84d9f9e0246a52e3989bf3b78a13bbbf05
readonly EXPECTED_LOCK_SHA256=b43ddef9c7a6b7e4c33cb6a8a4dd3659f26a7645a0f0c09782e04b2ee5b898a4
readonly EXPECTED_RESOLV_SHA256=b6874ac2e7b3c8ca61c0fa8a8e67646b79ad6024c93b7864576b44be1b6d0fa9
readonly EXPECTED_MISE_SHA256=2fd6b7eb9889f13e07dc3fdb6271c05815b81e03da86ff9f875de0b5c473afb8
readonly EXPECTED_GIT_SHA256=356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60
readonly EXPECTED_NAMESPACE_SHA256=122e35a9d762310b8cf8d8961d5d313288ce35439bc3bddb6ecd5d259894ab9b
readonly REGISTRY_PACKAGE_COUNT=74

fail() {
  printf 'cargo source fetch: %s\n' "$*" >&2
  exit 1
}

verify_host_tools() {
  local path expected actual
  while read -r path expected; do
    [[ -f $path && -x $path ]] || fail "required host tool is unavailable: $path"
    actual=$(/usr/bin/sha256sum "$path")
    actual=${actual%% *}
    [[ $actual == "$expected" ]] || fail "host tool digest mismatch: $path"
  done <<'TOOLS'
/usr/bin/awk eff9f2b711eb5bee1b58d4d33b891e8da8f6534655d3854a45bd10217dbb28e6
/usr/bin/bash 223fe8564b60636bc738b6178d3ba9a50ef7d791266b0efae6363bb716e4c47f
/usr/bin/cut f7f5511ede2187fbb7646581470c84647d087fb87cb74836c0237b0943d3dbc2
/usr/bin/env eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e
/usr/bin/find 91b75e72c495e68dd6aab198c2fd6c458fd51a2dc361045ee31f8e657417de26
/usr/bin/findmnt 598a23e36910f5cf7a99dfe83b6b29b54dae26ec73faa82fb097c0d5ddc36334
/usr/bin/hostname a9f8d0b870f2184ac9f51fc21d7951a8543349cffdc8cd6904c23d2485682220
/usr/bin/id 50629fa0ab5c003b538d4ccf7de34e85d2df4e799a79eb2dd4c2f7b1e0d7ca59
/usr/bin/mount 2f36342bd2fef42343c4fa4a2a396a7f56c7c8f4ddcbd31c43205c62e152e6d4
/usr/bin/pgrep 23bb9ae1e4b5ab7e11558429d1ba2bba77b757da52cfc780382c668342b500b3
/usr/bin/python3 889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9
/usr/bin/sha256sum 89f8c1d1ba3c76138f3771e1a91e2796ade6180b1c1e4258c04698ff32787c97
/usr/bin/stat 128754b37ab743a539d889a91441c440f9b52df77f4e008b345ff221ce1daeaa
/usr/bin/timeout 6ca1891dfc0b05d7680770c2884c0391b92467c7bb6500a5c84677e6481739f1
/usr/bin/unshare e7d3961bd874d5a4fe531efe6feb61b358b8000bd8c5fe91c5f1bdd9af81a002
/usr/sbin/chroot 019aa72e5412a6b0efdd6d94135acbc15ffbd37f7a5fb5ac47a9ef1c515c6c3e
TOOLS
}

git_safe() {
  /usr/bin/env -i PATH=/usr/bin:/bin GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
    GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 \
    "$ROOTFS/tools/bin/git" -c core.fsmonitor=false -c core.hooksPath=/dev/null \
    -c diff.external= -C "$SOURCE_DIR" "$@"
}

[[ $# == 0 ]] || fail 'takes no arguments'
verify_host_tools
[[ $(/usr/bin/id -u) == 0 ]] || fail 'requires effective uid 0'
[[ $(/usr/bin/hostname -s) == bastion ]] || fail 'host is not bastion'
[[ -d $TASK_DIR && ! -L $TASK_DIR ]] || fail 'task directory is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$TASK_DIR") == 0:0:700 ]] || fail 'task directory identity changed'
[[ -d $SOURCE_DIR && ! -L $SOURCE_DIR ]] || fail 'source directory is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$SOURCE_DIR") == 0:0:755 ]] || fail 'source directory identity changed'
[[ -d $ROOTFS && ! -L $ROOTFS ]] || fail 'reviewed rootfs is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$ROOTFS") == 0:0:755 ]] || fail 'rootfs identity changed'
[[ -f $BASE_MANIFEST && ! -L $BASE_MANIFEST ]] || fail 'base rootfs manifest is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$BASE_MANIFEST") == 0:0:600 ]] || fail 'base rootfs manifest identity changed'
[[ $(/usr/bin/sha256sum "$BASE_MANIFEST" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_BASE_MANIFEST_SHA256" ]] || fail 'base rootfs manifest digest mismatch'
[[ -x $ROOTFS/tools/mise/bin/mise && ! -L $ROOTFS/tools/mise/bin/mise ]] || fail 'pinned Mise binary is unavailable'
[[ $(/usr/bin/sha256sum "$ROOTFS/tools/mise/bin/mise" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_MISE_SHA256" ]] || fail 'pinned Mise binary digest mismatch'
[[ -x $ROOTFS/tools/bin/git && ! -L $ROOTFS/tools/bin/git ]] || fail 'pinned Git binary is unavailable'
[[ $(/usr/bin/sha256sum "$ROOTFS/tools/bin/git" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_GIT_SHA256" ]] || fail 'pinned Git binary digest mismatch'
[[ -x $ROOTFS/tools/mise-data/installs/rust/1.98.1/bin/cargo ]] || fail 'pinned Rust toolchain is unavailable'
[[ -d $ROOTFS/input && -d $ROOTFS/work && -d $ROOTFS/proc ]] || fail 'rootfs mount points are missing'
[[ -f $ROOTFS/etc/resolv.conf && ! -L $ROOTFS/etc/resolv.conf ]] || fail 'rootfs resolver input is unsafe'
[[ -f /etc/resolv.conf && ! -L /etc/resolv.conf ]] || fail 'host resolver is not a regular file'
[[ $(/usr/bin/stat -c '%u:%g:%a:%s' /etc/resolv.conf) == 0:0:644:167 ]] || fail 'host resolver identity changed'
[[ $(/usr/bin/sha256sum /etc/resolv.conf | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RESOLV_SHA256" ]] || fail 'host resolver digest mismatch'
[[ -f $NAMESPACE_SCRIPT && ! -L $NAMESPACE_SCRIPT ]] || fail 'namespace script is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$NAMESPACE_SCRIPT") == 0:0:700 ]] || fail 'namespace script identity changed'
[[ $EXPECTED_NAMESPACE_SHA256 =~ ^[0-9a-f]{64}$ ]] || fail 'namespace script hash is not sealed'
[[ $(/usr/bin/sha256sum "$NAMESPACE_SCRIPT" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_NAMESPACE_SHA256" ]] || fail 'namespace script digest mismatch'
[[ -d $LOG_DIR && ! -L $LOG_DIR ]] || fail 'private log directory is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$LOG_DIR") == 0:0:700 ]] || fail 'private log directory identity changed'
[[ ! -e $WORK_DIR && ! -L $WORK_DIR ]] || fail 'cargo work directory already exists'
[[ ! -e $LOG_FILE && ! -L $LOG_FILE ]] || fail 'cargo log already exists'
[[ ! -e $MANIFEST_FILE && ! -L $MANIFEST_FILE ]] || fail 'cargo seed manifest already exists'

readonly SOURCE_ORIGIN=https://github.com/tailrocks/velnor-new.git
readonly SOURCE_BRANCH=fix/generator-release-provenance
source_commit=$(git_safe rev-parse HEAD)
source_tree=$(git_safe rev-parse 'HEAD^{tree}')
main_parent=$(git_safe rev-parse 'HEAD^2')
source_branch=$(git_safe branch --show-current)
source_origin=$(git_safe remote get-url origin)
source_status=$(git_safe status --porcelain=v1 --untracked-files=all)
[[ $source_commit == "$EXPECTED_SOURCE_COMMIT" ]] || fail 'source commit does not match reviewed merge head'
[[ $source_tree == "$EXPECTED_SOURCE_TREE" ]] || fail 'source tree does not match reviewed merge tree'
[[ $main_parent == "$EXPECTED_MAIN_PARENT" ]] || fail 'source does not contain the exact fetched main parent'
[[ $source_branch == "$SOURCE_BRANCH" ]] || fail 'source branch identity changed'
[[ $source_origin == "$SOURCE_ORIGIN" ]] || fail 'repository origin changed'
[[ -z $source_status ]] || fail 'source worktree has local changes'
[[ ! -e $SOURCE_DIR/.cargo && ! -L $SOURCE_DIR/.cargo ]] || fail 'source adds Cargo configuration'
[[ $(/usr/bin/sha256sum "$SOURCE_DIR/Cargo.lock" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_LOCK_SHA256" ]] || fail 'source lockfile digest mismatch'

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$BASE_MANIFEST" "$ROOTFS" "$EXPECTED_BASE_MANIFEST_SHA256" "$EXPECTED_BASE_ROOTFS_TREE_SHA256" "$EXPECTED_PLAN_SHA256" "$EXPECTED_TOOL_PROVENANCE_SHA256" <<'PY'
import hashlib
import json
import os
import posixpath
import stat
import sys

manifest_path, rootfs, manifest_sha, tree_sha, plan_sha, provenance_sha = sys.argv[1:]

def fail(message):
    raise SystemExit("cargo source fetch: " + message)

st = os.lstat(manifest_path)
if not stat.S_ISREG(st.st_mode) or st.st_uid != 0 or st.st_gid != 0 or st.st_nlink != 1 or stat.S_IMODE(st.st_mode) != 0o600:
    fail("base rootfs manifest is not a safe private regular file")
data = open(manifest_path, "rb").read()
if hashlib.sha256(data).hexdigest() != manifest_sha:
    fail("base rootfs manifest digest mismatch")
manifest = json.loads(data)
if manifest != {
    "base_rootfs_tree_sha256": tree_sha,
    "materializer_sha256": "4aba299ea3cb925bae7413bd6b8b45845d1ce83a1f617d964b9068dbf87da0a3",
    "plan_sha256": plan_sha,
    "schema": 1,
    "status": "BASE_MATERIALIZED; cargo seed and final stage manifest are still absent",
    "tool_provenance_sha256": provenance_sha,
}:
    fail("base rootfs manifest fields differ from the reviewed stage")

records = []

def walk(fd, prefix):
    for name in sorted(os.listdir(fd), key=os.fsencode):
        if name in (".", "..") or "/" in name:
            fail("invalid rootfs directory entry")
        path = (prefix + "/" + name) if prefix else "/" + name
        item = os.stat(name, dir_fd=fd, follow_symlinks=False)
        mode = stat.S_IMODE(item.st_mode)
        if item.st_uid != 0 or item.st_gid != 0:
            fail("rootfs owner mismatch")
        if stat.S_ISCHR(item.st_mode) and path == "/dev/null" and os.major(item.st_rdev) == 1 and os.minor(item.st_rdev) == 3:
            if mode != 0o666:
                fail("unexpected null device mode")
            records.append({"path": path, "type": "character-device", "mode": mode, "uid": 0, "gid": 0, "major": 1, "minor": 3})
            continue
        if stat.S_ISLNK(item.st_mode):
            target = os.readlink(name, dir_fd=fd)
            normalized = posixpath.normpath(target if target.startswith("/") else posixpath.join(posixpath.dirname(path), target))
            if not normalized.startswith("/") or normalized == "/.." or normalized.startswith("/../"):
                fail("rootfs symlink escape")
            records.append({"path": path, "type": "symlink", "mode": mode, "uid": 0, "gid": 0, "target": target})
            continue
        if mode & 0o022:
            fail("rootfs writable-by-group-or-world path")
        if stat.S_ISDIR(item.st_mode):
            child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            try:
                opened = os.fstat(child)
                if not stat.S_ISDIR(opened.st_mode) or (opened.st_dev, opened.st_ino) != (item.st_dev, item.st_ino):
                    fail("rootfs directory changed during hashing")
                records.append({"path": path, "type": "directory", "mode": mode, "uid": 0, "gid": 0})
                walk(child, path)
            finally:
                os.close(child)
        elif stat.S_ISREG(item.st_mode):
            if item.st_nlink != 1:
                fail("rootfs hard link rejected")
            child = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            try:
                opened = os.fstat(child)
                if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (item.st_dev, item.st_ino):
                    fail("rootfs file changed during hashing")
                digest = hashlib.sha256()
                while True:
                    block = os.read(child, 1 << 20)
                    if not block:
                        break
                    digest.update(block)
                records.append({"path": path, "type": "file", "mode": mode, "uid": 0, "gid": 0, "size": item.st_size, "sha256": digest.hexdigest()})
            finally:
                os.close(child)
        else:
            fail("unsupported rootfs entry")

rootfd = os.open(rootfs, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
rootst = os.fstat(rootfd)
if not stat.S_ISDIR(rootst.st_mode) or rootst.st_uid != 0 or rootst.st_gid != 0 or stat.S_IMODE(rootst.st_mode) != 0o755:
    os.close(rootfd)
    fail("rootfs root mode mismatch")
records.append({"path": "/", "type": "directory", "mode": 0o755, "uid": 0, "gid": 0})
try:
    walk(rootfd, "")
finally:
    os.close(rootfd)
encoded = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
if hashlib.sha256(encoded).hexdigest() != tree_sha:
    fail("rootfs tree digest mismatch")
print("reviewed base rootfs manifest and complete tree verified")
PY

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$SOURCE_DIR/Cargo.lock" "$EXPECTED_LOCK_SHA256" "$REGISTRY_PACKAGE_COUNT" <<'PY'
import hashlib
import re
import sys
import tomllib

path, expected_sha, expected_count = sys.argv[1:]
data = open(path, "rb").read()
if hashlib.sha256(data).hexdigest() != expected_sha:
    raise SystemExit("cargo source fetch: lockfile digest mismatch")
lock = tomllib.loads(data.decode("utf-8"))
packages = lock.get("package")
if not isinstance(packages, list):
    raise SystemExit("cargo source fetch: lock package table is missing")
registry = []
git_sources = []
other_sources = []
for package in packages:
    source = package.get("source")
    if source is None:
        continue
    if source == "registry+https://github.com/rust-lang/crates.io-index":
        name = package.get("name")
        version = package.get("version")
        checksum = package.get("checksum")
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name):
            raise SystemExit("cargo source fetch: invalid registry package name")
        if not isinstance(version, str) or not re.fullmatch(r"[A-Za-z0-9.+_-]+", version):
            raise SystemExit("cargo source fetch: invalid registry package version")
        if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise SystemExit("cargo source fetch: invalid registry package checksum")
        registry.append((name, version, checksum))
    elif source.startswith("git+"):
        git_sources.append(source)
    else:
        other_sources.append(source)
if len(registry) != int(expected_count) or git_sources or other_sources:
    raise SystemExit("cargo source fetch: lock source inventory differs from the reviewed crates.io-only set")
if len({(name, version) for name, version, _ in registry}) != len(registry):
    raise SystemExit("cargo source fetch: duplicate registry package identity")
print(f"lock inventory verified: {len(registry)} crates.io packages; 0 Git packages")
PY

if /usr/bin/pgrep -u 65534 >/dev/null 2>&1; then fail 'uid 65534 is already active on the host'; fi
if /usr/bin/findmnt -rn -o TARGET | /usr/bin/awk -v p="$TASK_DIR" '$0==p || index($0,p"/")==1 { found=1 } END { exit !found }'; then
  fail 'task directory already has a mounted resource'
fi

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$WORK_DIR" "$LOG_FILE" <<'PY'
import os
import sys

work, log = sys.argv[1:]
os.mkdir(work, 0o755)
os.chown(work, 0, 0)
os.chmod(work, 0o755)
for name in ("home", "tmp", "rustup", "mise-cache", "xdg-data", "xdg-cache", "xdg-state", "xdg-config", "cargo-seed"):
    path = os.path.join(work, name)
    os.mkdir(path, 0o700)
    os.chown(path, 65534, 65534)
    os.chmod(path, 0o700)
fd = os.open(log, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
try:
    os.fchown(fd, 0, 0)
    os.fchmod(fd, 0o600)
finally:
    os.close(fd)
PY
exec </dev/null >>"$LOG_FILE" 2>&1
printf 'source commit: %s\nsource tree: %s\nmain parent: %s\nlock sha256: %s\n' \
  "$source_commit" "$source_tree" "$main_parent" "$EXPECTED_LOCK_SHA256"
printf 'cargo fetch command: mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo fetch --locked --manifest-path /input/Cargo.toml\n'

fetch_status=0
/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/timeout --foreground --signal=TERM --kill-after=10s 900s \
  /usr/bin/python3 -I -S - "$NAMESPACE_SCRIPT" <<'PY' || fetch_status=$?
import os
import resource
import sys

script = sys.argv[1]
limits = (
    (resource.RLIMIT_CPU, 600),
    (resource.RLIMIT_FSIZE, 1 << 29),
    (resource.RLIMIT_NPROC, 64),
    (resource.RLIMIT_NOFILE, 64),
    (resource.RLIMIT_AS, 4 << 30),
    (resource.RLIMIT_CORE, 0),
)
for limit, value in limits:
    resource.setrlimit(limit, (value, value))
net_ns = os.readlink("/proc/self/ns/net")
expected = {
    "PATH": "/usr/sbin:/usr/bin:/sbin:/bin",
    "EXPECTED_MNT_NS": os.readlink("/proc/self/ns/mnt"),
    "EXPECTED_PID_NS": os.readlink("/proc/self/ns/pid"),
    "EXPECTED_IPC_NS": os.readlink("/proc/self/ns/ipc"),
    "EXPECTED_UTS_NS": os.readlink("/proc/self/ns/uts"),
    "EXPECTED_NET_NS": net_ns,
}
null_fd = os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC)
os.dup2(null_fd, 0)
if null_fd > 2:
    os.close(null_fd)
os.close_range(3, 0xFFFFFFFF)
argv = [
    "/usr/bin/unshare", "--mount", "--pid", "--ipc", "--uts", "--fork",
    "--kill-child", "--propagation", "private", "--",
    "/usr/bin/bash", "--noprofile", "--norc", script,
]
os.execve(argv[0], argv, expected)
PY

cleanup_status=0
if /usr/bin/pgrep -u 65534 >/dev/null 2>&1; then
  printf 'cargo source fetch: uid 65534 process remains after namespace exit\n' >&2
  cleanup_status=1
fi
if /usr/bin/findmnt -rn -o TARGET | /usr/bin/awk -v p="$TASK_DIR" '$0==p || index($0,p"/")==1 { found=1 } END { exit !found }'; then
  printf 'cargo source fetch: task-owned mount remains after namespace exit\n' >&2
  cleanup_status=1
fi
[[ $cleanup_status == 0 ]] || fail 'namespace cleanup check failed; preserve private logs and work for review'
[[ $fetch_status == 0 ]] || fail "Cargo fetch failed with status $fetch_status; preserve private logs and work for review"

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$SOURCE_DIR/Cargo.lock" "$WORK_DIR/cargo-seed" "$MANIFEST_FILE" "$EXPECTED_SOURCE_COMMIT" "$EXPECTED_SOURCE_TREE" "$EXPECTED_MAIN_PARENT" "$EXPECTED_LOCK_SHA256" "$REGISTRY_PACKAGE_COUNT" <<'PY'
import hashlib
import json
import os
import re
import stat
import sys
import tomllib

lock_path, cargo_home, manifest_path, commit, tree, main_parent, lock_sha, count_text = sys.argv[1:]
expected_count = int(count_text)

def fail(message):
    raise SystemExit("cargo source fetch: " + message)

def read_lock():
    st = os.lstat(lock_path)
    if not stat.S_ISREG(st.st_mode) or st.st_uid != 0 or st.st_gid != 0 or st.st_nlink != 1 or stat.S_IMODE(st.st_mode) & 0o022:
        fail("lockfile is not a safe root-owned regular file")
    data = open(lock_path, "rb").read()
    if hashlib.sha256(data).hexdigest() != lock_sha:
        fail("lockfile changed after fetch")
    packages = tomllib.loads(data.decode("utf-8")).get("package")
    if not isinstance(packages, list):
        fail("lock package table is missing")
    registry = {}
    for package in packages:
        source = package.get("source")
        if source is None:
            continue
        if source != "registry+https://github.com/rust-lang/crates.io-index":
            fail("lock contains a non-crates.io source")
        name = package.get("name")
        version = package.get("version")
        checksum = package.get("checksum")
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name):
            fail("invalid registry package name")
        if not isinstance(version, str) or not re.fullmatch(r"[A-Za-z0-9.+_-]+", version):
            fail("invalid registry package version")
        if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            fail("invalid registry package checksum")
        filename = f"{name}-{version}.crate"
        if filename in registry:
            fail("duplicate registry archive filename")
        registry[filename] = {"name": name, "version": version, "sha256": checksum}
    if len(registry) != expected_count:
        fail("locked crates.io package count changed")
    return registry

def walk_tree(root):
    root_st = os.lstat(root)
    if not stat.S_ISDIR(root_st.st_mode) or root_st.st_uid != 65534 or root_st.st_gid != 65534 or stat.S_IMODE(root_st.st_mode) != 0o700:
        fail("Cargo home root identity changed")
    records = [{"path": ".", "type": "dir", "mode": 0o700, "uid": 65534, "gid": 65534}]
    total = 0
    for current, dirs, files in os.walk(root, topdown=True, followlinks=False):
        dirs.sort(key=os.fsencode)
        files.sort(key=os.fsencode)
        for name in list(dirs):
            path = os.path.join(current, name)
            st = os.lstat(path)
            if not stat.S_ISDIR(st.st_mode) or st.st_uid != 65534 or st.st_gid != 65534 or stat.S_IMODE(st.st_mode) & 0o022:
                fail("Cargo home contains an unsafe directory")
            rel = os.path.relpath(path, root)
            records.append({"path": rel, "type": "dir", "mode": stat.S_IMODE(st.st_mode), "uid": st.st_uid, "gid": st.st_gid})
        for name in files:
            path = os.path.join(current, name)
            st = os.lstat(path)
            if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or st.st_uid != 65534 or st.st_gid != 65534 or stat.S_IMODE(st.st_mode) & 0o022:
                fail("Cargo home contains a non-regular or unsafe file")
            if st.st_size > (1 << 28):
                fail("Cargo home file exceeds the size limit")
            total += st.st_size
            if total > (1 << 29):
                fail("Cargo home exceeds the total size limit")
            digest = hashlib.sha256()
            with open(path, "rb") as stream:
                for block in iter(lambda: stream.read(1 << 20), b""):
                    digest.update(block)
            rel = os.path.relpath(path, root)
            records.append({"path": rel, "type": "file", "mode": stat.S_IMODE(st.st_mode), "uid": st.st_uid, "gid": st.st_gid, "size": st.st_size, "sha256": digest.hexdigest()})
    records.sort(key=lambda item: os.fsencode(item["path"]))
    encoded = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
    return hashlib.sha256(encoded).hexdigest(), total

registry = read_lock()
target_root = os.path.join(os.path.dirname(cargo_home), "target")
if os.path.lexists(target_root):
    fail("Cargo target output exists; this phase permits no compilation")
for blocked in ("credentials", "credentials.toml", "config", "config.toml"):
    if os.path.lexists(os.path.join(cargo_home, blocked)):
        fail("Cargo home contains an authentication or source configuration file")
if os.path.lexists(os.path.join(cargo_home, "git")) or os.path.lexists(os.path.join(cargo_home, "registry", "src")):
    fail("Cargo fetch created Git state or unpacked source trees")
cache_root = os.path.join(cargo_home, "registry", "cache")
if not os.path.isdir(cache_root) or os.path.islink(cache_root):
    fail("Cargo registry archive cache is missing")
archives = {}
for current, dirs, files in os.walk(cache_root, topdown=True, followlinks=False):
    dirs.sort(key=os.fsencode)
    files.sort(key=os.fsencode)
    for directory in dirs:
        if os.path.islink(os.path.join(current, directory)):
            fail("registry cache contains a symlink")
    for filename in files:
        if not filename.endswith(".crate"):
            continue
        path = os.path.join(current, filename)
        st = os.lstat(path)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1:
            fail("registry cache archive is not a regular file")
        if filename in archives:
            fail("duplicate registry cache archive")
        archives[filename] = path
if set(archives) != set(registry):
    fail("downloaded archive set differs from the lockfile")
for filename, expected in registry.items():
    path = archives[filename]
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    if digest.hexdigest() != expected["sha256"]:
        fail("downloaded archive checksum differs from Cargo.lock")
if os.path.lexists(manifest_path):
    fail("Cargo seed manifest already exists")
seed_tree_sha, total_bytes = walk_tree(cargo_home)
manifest = {
    "schema": 1,
    "status": "FETCHED; offline cargo metadata gate NOT RUN; generator eligibility NOT ESTABLISHED",
    "repository": "https://github.com/tailrocks/velnor-new",
    "source_commit": commit,
    "source_tree": tree,
    "source_main_parent": main_parent,
    "lock_path": "Cargo.lock",
    "lock_sha256": lock_sha,
    "registry": "crates.io",
    "registry_package_count": expected_count,
    "git_package_count": 0,
    "archive_count": len(archives),
    "network_namespace": "same as host; Cargo access is constrained by the exact locked registry-only command and absence of credentials, but kernel egress destinations are not allowlisted",
    "offline_metadata_gate": "NOT RUN",
    "cargo_home_tree_sha256": seed_tree_sha,
    "cargo_home_bytes": total_bytes,
    "archives": [registry[name] for name in sorted(registry)],
}
data = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode()
fd = os.open(manifest_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
try:
    if os.write(fd, data) != len(data):
        fail("short manifest write")
    os.fsync(fd)
finally:
    os.close(fd)
print(f"Cargo seed verified: {expected_count} crates.io archives; 0 Git packages; lock {lock_sha}; tree {seed_tree_sha}")
PY
printf 'Private Cargo source seed is verified. No Cargo build or helper execution ran.\n'

#!/usr/bin/env -S -i PATH=/usr/bin:/bin /usr/bin/bash --noprofile --norc
# shellcheck shell=bash
# External launch: env -i PATH=/usr/bin:/bin bash --noprofile --norc /root/.velnor-generator-preview-20261005/preseed-preview-cargo-fetch-v2.sh
# Fetch only the exact lockfile's public crates.io archives in the private task rootfs.
set -Eeuo pipefail
umask 077

readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly ROOTFS="$TASK_DIR/rootfs"
readonly SOURCE_DIR="$TASK_DIR/cargo-fetch-bc7-source"
readonly OBJECTS_DIR=/root/Projects/tailrocks/velnor-project/velnor-new/.git/objects
readonly SOURCE_MANIFEST="$TASK_DIR/cargo-fetch-bc7-source-manifest.json"
readonly WORK_DIR="$TASK_DIR/cargo-fetch-bc7-work"
readonly SAFE_GIT_DIR="$WORK_DIR/git-reader"
readonly LOG_DIR="$TASK_DIR/logs"
readonly LOG_FILE="$LOG_DIR/cargo-fetch-bc7.log"
readonly MANIFEST_FILE="$TASK_DIR/cargo-source-seed-bc7-manifest.json"
readonly BASE_MANIFEST="$TASK_DIR/rootfs-base-manifest.json"
readonly EXPECTED_BASE_MANIFEST_SHA256=1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf
readonly EXPECTED_BASE_ROOTFS_TREE_SHA256=1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763
readonly EXPECTED_PLAN_SHA256=ebca0ef5f4c716e533cece9637b3d68763cc8265ce4016bf84a85a279416e77f
readonly EXPECTED_TOOL_PROVENANCE_SHA256=67a8ba245858b11276a6496a513584fefec26e26218b0cdfad028a3fac07d137
readonly NAMESPACE_SCRIPT="$TASK_DIR/preseed-preview-cargo-fetch-namespace-v2.sh"
readonly EXPECTED_SOURCE_COMMIT=bc7f784a51233bd29676353392c9fc07c0ad01a7
readonly EXPECTED_SOURCE_TREE=b3939a4d6010f243485fb528f73af74d49b2fb94
readonly EXPECTED_BRANCH_PARENT=ff41745394712604bddf94b6a4e9f1069100789e
readonly EXPECTED_MAIN_PARENT=703d1b84d9f9e0246a52e3989bf3b78a13bbbf05
readonly EXPECTED_LOCK_SHA256=b43ddef9c7a6b7e4c33cb6a8a4dd3659f26a7645a0f0c09782e04b2ee5b898a4
readonly EXPECTED_RESOLV_SHA256=b6874ac2e7b3c8ca61c0fa8a8e67646b79ad6024c93b7864576b44be1b6d0fa9
readonly EXPECTED_MISE_SHA256=2fd6b7eb9889f13e07dc3fdb6271c05815b81e03da86ff9f875de0b5c473afb8
readonly EXPECTED_GIT_SHA256=356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60
readonly EXPECTED_NAMESPACE_SHA256=d66c3a5c826a4a3916281324a52e83d41a4ea463db7200da720f6d122bfefb25
readonly REGISTRY_PACKAGE_COUNT=74

fail() {
  printf 'cargo source fetch: %s\n' "$*" >&2
  exit 1
}

close_inherited_fds() {
  local descriptor
  local -a inherited_descriptors remaining_descriptors
  mapfile -t inherited_descriptors < <(/usr/bin/find "/proc/$$/fd" -mindepth 1 -maxdepth 1 -printf '%f\n')
  for descriptor in "${inherited_descriptors[@]}"; do
    case "$descriptor" in
      0|1|2) continue ;;
      ''|*[!0-9]*) fail 'cannot identify an inherited file descriptor' ;;
    esac
    if [[ "/proc/$$/fd/$descriptor" -ef $0 ]]; then
      continue
    fi
    exec {descriptor}>&- || fail 'cannot close an inherited file descriptor'
  done
  mapfile -t remaining_descriptors < <(/usr/bin/find "/proc/$$/fd" -mindepth 1 -maxdepth 1 -printf '%f\n')
  for descriptor in "${remaining_descriptors[@]}"; do
    case "$descriptor" in
      0|1|2) ;;
      ''|*[!0-9]*) fail 'cannot verify inherited file descriptors' ;;
      *) [[ ! -e "/proc/$$/fd/$descriptor" || "/proc/$$/fd/$descriptor" -ef $0 ]] || fail 'an inherited file descriptor remains open' ;;
    esac
  done
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
/usr/bin/readlink 1dcaf494f4dec175292b1684b3e978d18a4c3c703c52f81606d9e8eea30eb6db
/usr/bin/python3 889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9
/usr/bin/sha256sum 89f8c1d1ba3c76138f3771e1a91e2796ade6180b1c1e4258c04698ff32787c97
/usr/bin/stat 128754b37ab743a539d889a91441c440f9b52df77f4e008b345ff221ce1daeaa
/usr/bin/timeout 6ca1891dfc0b05d7680770c2884c0391b92467c7bb6500a5c84677e6481739f1
/usr/bin/unshare e7d3961bd874d5a4fe531efe6feb61b358b8000bd8c5fe91c5f1bdd9af81a002
/usr/sbin/chroot 019aa72e5412a6b0efdd6d94135acbc15ffbd37f7a5fb5ac47a9ef1c515c6c3e
TOOLS
}

[[ ${PATH:-} == /usr/bin:/bin ]] || fail 'outer Bash did not start with the sanitized environment'
verify_host_tools
[[ $# == 1 ]] || fail 'fixture requires one fd mode'
case "$1" in
  clean) [[ ! -e "/proc/$BASHPID/fd/3" ]] || fail 'clean wrapper inherited fd 3' ;;
  inherited) [[ -e "/proc/$BASHPID/fd/3" ]] || fail 'inherited wrapper lacks fd 3' ;;
  *) fail 'unknown fixture fd mode' ;;
esac
close_inherited_fds

[[ $(/usr/bin/id -u) == 0 ]] || fail 'requires effective uid 0'
[[ $(/usr/bin/hostname -s) == bastion ]] || fail 'host is not bastion'
[[ -d $TASK_DIR && ! -L $TASK_DIR ]] || fail 'task directory is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$TASK_DIR") == 0:0:700 ]] || fail 'task directory identity changed'
[[ ! -e $SOURCE_DIR && ! -L $SOURCE_DIR ]] || fail 'source snapshot already exists'
[[ ! -e $SOURCE_MANIFEST && ! -L $SOURCE_MANIFEST ]] || fail 'source manifest already exists'
[[ -d $OBJECTS_DIR && ! -L $OBJECTS_DIR ]] || fail 'trusted Git object directory is unavailable'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$OBJECTS_DIR") == 0:0:755 ]] || fail 'trusted Git object directory identity changed'
[[ ! -e $OBJECTS_DIR/info/alternates && ! -L $OBJECTS_DIR/info/alternates ]] || fail 'Git object alternates are not permitted'
[[ -d $ROOTFS && ! -L $ROOTFS ]] || fail 'reviewed rootfs is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$ROOTFS") == 0:0:755 ]] || fail 'rootfs identity changed'
[[ -f $BASE_MANIFEST && ! -L $BASE_MANIFEST ]] || fail 'base rootfs manifest is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$BASE_MANIFEST") == 0:0:600 ]] || fail 'base rootfs manifest identity changed'
[[ $(/usr/bin/sha256sum "$BASE_MANIFEST" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_BASE_MANIFEST_SHA256" ]] || fail 'base rootfs manifest digest mismatch'
[[ -x $ROOTFS/tools/mise/bin/mise && ! -L $ROOTFS/tools/mise/bin/mise ]] || fail 'pinned Mise binary is unavailable'
[[ $(/usr/bin/sha256sum "$ROOTFS/tools/mise/bin/mise" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_MISE_SHA256" ]] || fail 'pinned Mise binary digest mismatch'
[[ -x $ROOTFS/tools/bin/git && ! -L $ROOTFS/tools/bin/git ]] || fail 'pinned Git binary is unavailable'
[[ $(/usr/bin/sha256sum "$ROOTFS/tools/bin/git" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_GIT_SHA256" ]] || fail 'pinned Git binary digest mismatch'
rust_toolchain_link="$ROOTFS/tools/mise-data/installs/rust/1.98.1"
[[ -L $rust_toolchain_link && $(/usr/bin/readlink "$rust_toolchain_link") == /tools/rust-1.98.1 ]] || fail 'pinned Mise Rust selector link differs from the reviewed toolchain'
[[ -x $ROOTFS/tools/rust-1.98.1/bin/cargo && -x $ROOTFS/tools/rust-1.98.1/bin/rustc ]] || fail 'pinned Rust toolchain binaries are unavailable'
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

if /usr/bin/pgrep -u 65534 >/dev/null 2>&1; then fail 'uid 65534 is already active on the host'; fi
if /usr/bin/findmnt -rn -o TARGET | /usr/bin/awk -v p="$TASK_DIR" '$0==p || index($0,p"/")==1 { found=1 } END { exit !found }'; then
  fail 'task directory already has a mounted resource'
fi

printf 'complete sealed outer preflight PASS (%s)\n' "$1"

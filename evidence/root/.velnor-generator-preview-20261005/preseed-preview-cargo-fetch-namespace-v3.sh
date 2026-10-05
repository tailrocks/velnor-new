#!/usr/bin/bash
# Mount one reviewed source and task cache before dropping all capabilities.
set -Eeuo pipefail
umask 077

readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly ROOTFS="$TASK_DIR/rootfs"
readonly SOURCE_DIR="$TASK_DIR/cargo-fetch-bc7-source-attempt-2"
readonly SOURCE_MANIFEST="$TASK_DIR/cargo-fetch-bc7-source-attempt-2-manifest.json"
readonly WORK_DIR="$TASK_DIR/cargo-fetch-bc7-work-attempt-2"
readonly HOME_HELPER="$TASK_DIR/rustup-home-prepare-v2.py"
readonly HOME_HELPER_SHA256=5876a1cfb5a1c6a43182995af93a31cb8729fa4421e3c5466a1afe8cfb9e3f0f
readonly EXPECTED_SOURCE_COMMIT=bc7f784a51233bd29676353392c9fc07c0ad01a7
readonly EXPECTED_SOURCE_TREE=b3939a4d6010f243485fb528f73af74d49b2fb94
readonly EXPECTED_BRANCH_PARENT=ff41745394712604bddf94b6a4e9f1069100789e
readonly EXPECTED_MAIN_PARENT=703d1b84d9f9e0246a52e3989bf3b78a13bbbf05
readonly EXPECTED_LOCK_SHA256=b43ddef9c7a6b7e4c33cb6a8a4dd3659f26a7645a0f0c09782e04b2ee5b898a4

fail() {
  printf 'cargo source namespace: %s\n' "$*" >&2
  exit 1
}

[[ $# == 0 ]] || fail 'takes no arguments'
[[ $(/usr/bin/id -u) == 0 ]] || fail 'namespace setup requires uid 0'
[[ ${PATH:-} == /usr/sbin:/usr/bin:/sbin:/bin ]] || fail 'host PATH is not sanitized'
for name in EXPECTED_MNT_NS EXPECTED_PID_NS EXPECTED_IPC_NS EXPECTED_UTS_NS EXPECTED_NET_NS; do
  [[ -n ${!name:-} ]] || fail 'host namespace identity is missing'
done
[[ ${SOURCE_MANIFEST_SHA256:-} =~ ^[0-9a-f]{64}$ ]] || fail 'source manifest digest is missing'

[[ -d $SOURCE_DIR && ! -L $SOURCE_DIR ]] || fail 'source is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$SOURCE_DIR") == 0:0:755 ]] || fail 'source snapshot identity changed'
[[ -f $SOURCE_MANIFEST && ! -L $SOURCE_MANIFEST ]] || fail 'source manifest is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$SOURCE_MANIFEST") == 0:0:600 ]] || fail 'source manifest identity changed'

/usr/bin/env -i PATH=/usr/bin:/bin SOURCE_MANIFEST_SHA256="$SOURCE_MANIFEST_SHA256" \
  /usr/bin/python3 -I -S - "$SOURCE_DIR" "$SOURCE_MANIFEST" "$SOURCE_MANIFEST_SHA256" \
  "$EXPECTED_SOURCE_COMMIT" "$EXPECTED_SOURCE_TREE" "$EXPECTED_BRANCH_PARENT" "$EXPECTED_MAIN_PARENT" "$EXPECTED_LOCK_SHA256" <<'PY'
import hashlib
import json
import os
import posixpath
import stat
import sys

source, manifest_path, manifest_sha, commit, tree, branch_parent, main_parent, lock_sha = sys.argv[1:]

def fail(message):
    raise SystemExit("cargo source namespace: " + message)

manifest_stat = os.lstat(manifest_path)
if not stat.S_ISREG(manifest_stat.st_mode) or manifest_stat.st_uid != 0 or manifest_stat.st_gid != 0 or manifest_stat.st_nlink != 1 or stat.S_IMODE(manifest_stat.st_mode) != 0o600:
    fail("source manifest is not a safe private regular file")
manifest_bytes = open(manifest_path, "rb").read()
if hashlib.sha256(manifest_bytes).hexdigest() != manifest_sha:
    fail("source manifest digest changed")
manifest = json.loads(manifest_bytes)
if manifest.get("schema") != 1 or manifest.get("status") != "EXACT_OBJECT_SNAPSHOT_FOR_LOCKED_CARGO_FETCH_ONLY":
    fail("source manifest status is not eligible for this fetch")
if manifest.get("source_commit") != commit or manifest.get("source_tree") != tree or manifest.get("source_parents") != [branch_parent, main_parent]:
    fail("source commit/tree/parents differ from the reviewed PR merge")
if manifest.get("lock_sha256") != lock_sha or manifest.get("generator_input") is not False or manifest.get("repository") != "https://github.com/tailrocks/velnor-new":
    fail("source manifest contract differs from the reviewed Cargo-only source")
if manifest.get("cargo_configuration_policy") != "Only tracked .cargo/mutants.toml is included; .cargo/config and .cargo/config.toml are prohibited.":
    fail("source manifest does not declare the restricted Cargo configuration policy")
if os.path.lexists(os.path.join(source, ".git")):
    fail("Cargo snapshot contains Git metadata")
cargo_dir = os.lstat(os.path.join(source, ".cargo"))
mutants_path = os.path.join(source, ".cargo", "mutants.toml")
mutants_stat = os.lstat(mutants_path)
if not stat.S_ISDIR(cargo_dir.st_mode) or cargo_dir.st_uid != 0 or cargo_dir.st_gid != 0 or stat.S_IMODE(cargo_dir.st_mode) != 0o755:
    fail(".cargo directory is not the expected safe directory")
if not stat.S_ISREG(mutants_stat.st_mode) or mutants_stat.st_uid != 0 or mutants_stat.st_gid != 0 or mutants_stat.st_nlink != 1 or stat.S_IMODE(mutants_stat.st_mode) != 0o644:
    fail(".cargo/mutants.toml is not the expected regular file")
mutants_fd = os.open(mutants_path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
try:
    mutants_digest = hashlib.sha256()
    while True:
        block = os.read(mutants_fd, 1 << 20)
        if not block:
            break
        mutants_digest.update(block)
finally:
    os.close(mutants_fd)
if mutants_digest.hexdigest() != manifest.get("cargo_mutants_sha256"):
    fail(".cargo/mutants.toml digest differs from the source manifest")

records = []
def walk(directory, relative="."):
    item = os.lstat(directory)
    if not stat.S_ISDIR(item.st_mode) or item.st_uid != 0 or item.st_gid != 0 or stat.S_IMODE(item.st_mode) != 0o755:
        fail("snapshot directory identity or mode changed")
    records.append({"path": relative, "type": "directory", "mode": 0o755, "uid": 0, "gid": 0})
    with os.scandir(directory) as scan:
        entries = sorted(list(scan), key=lambda entry: os.fsencode(entry.name))
    for entry in entries:
        rel = entry.name if relative == "." else relative + "/" + entry.name
        if entry.name == ".git" or "/.git/" in rel:
            fail("Git metadata path in source snapshot")
        if entry.name == ".cargo" and rel != ".cargo":
            fail(".cargo directory is permitted only at source root")
        if rel == ".cargo":
            if not os.path.isdir(entry.path) or os.path.islink(entry.path):
                fail(".cargo is not a regular directory")
        elif rel == ".cargo/mutants.toml":
            if not os.path.isfile(entry.path) or os.path.islink(entry.path):
                fail(".cargo/mutants.toml is not a regular file")
        elif rel.startswith(".cargo/") or "/.cargo/" in rel:
            fail("only .cargo/mutants.toml is permitted under .cargo")
        current = os.lstat(entry.path)
        mode = stat.S_IMODE(current.st_mode)
        if current.st_uid != 0 or current.st_gid != 0 or (not stat.S_ISLNK(current.st_mode) and mode & 0o022):
            fail("snapshot entry ownership or mode changed")
        if stat.S_ISDIR(current.st_mode):
            walk(entry.path, rel)
        elif stat.S_ISREG(current.st_mode):
            if current.st_nlink != 1:
                fail("source snapshot contains a hard link")
            fd = os.open(entry.path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                opened = os.fstat(fd)
                if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (current.st_dev, current.st_ino):
                    fail("source file changed during verification")
                digest = hashlib.sha256()
                while True:
                    block = os.read(fd, 1 << 20)
                    if not block:
                        break
                    digest.update(block)
                records.append({"path": rel, "type": "file", "mode": mode, "uid": 0, "gid": 0, "size": current.st_size, "sha256": digest.hexdigest()})
            finally:
                os.close(fd)
        elif stat.S_ISLNK(current.st_mode):
            target = os.readlink(entry.path)
            if target.startswith("/") or "\\" in target or any(part == ".." for part in target.split("/")) or posixpath.normpath(posixpath.join(posixpath.dirname(rel), target)).startswith("../"):
                fail("source symlink escapes the snapshot")
            records.append({"path": rel, "type": "symlink", "mode": mode, "uid": 0, "gid": 0, "target": target})
        else:
            fail("unsupported source snapshot entry")
walk(source)
records.sort(key=lambda record: os.fsencode(record["path"]))
snapshot_bytes = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
if hashlib.sha256(snapshot_bytes).hexdigest() != manifest.get("cargo_snapshot_tree_sha256"):
    fail("source snapshot tree digest changed")
lock_path = os.path.join(source, "Cargo.lock")
lock_fd = os.open(lock_path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
try:
    lock_hash = hashlib.sha256()
    while True:
        block = os.read(lock_fd, 1 << 20)
        if not block:
            break
        lock_hash.update(block)
finally:
    os.close(lock_fd)
if lock_hash.hexdigest() != lock_sha:
    fail("Cargo.lock digest changed")
print("exact object-derived source snapshot and Cargo.lock verified")
PY

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$EXPECTED_MNT_NS" "$EXPECTED_PID_NS" "$EXPECTED_IPC_NS" "$EXPECTED_UTS_NS" "$EXPECTED_NET_NS" <<'PY'
import os
import sys

names = ("mnt", "pid", "ipc", "uts", "net")
for name, expected in zip(names, sys.argv[1:], strict=True):
    actual = os.readlink(f"/proc/self/ns/{name}")
    if name == "net":
        if actual != expected:
            raise SystemExit("cargo source namespace: network namespace changed")
    elif actual == expected:
        raise SystemExit(f"cargo source namespace: new {name} namespace was not created")
PY

[[ -d $ROOTFS && ! -L $ROOTFS ]] || fail 'rootfs is missing or symlinked'
[[ -d $ROOTFS/input && -d $ROOTFS/work && -d $ROOTFS/proc ]] || fail 'rootfs mount point is missing'
[[ -f $ROOTFS/etc/resolv.conf && ! -L $ROOTFS/etc/resolv.conf ]] || fail 'resolver mount point is unsafe'
[[ -d $SOURCE_DIR && ! -L $SOURCE_DIR ]] || fail 'source is missing or symlinked'
[[ -d $WORK_DIR && ! -L $WORK_DIR ]] || fail 'private work directory is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$WORK_DIR") == 65534:65534:700 ]] || fail 'private work directory identity changed'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$WORK_DIR/cargo-seed") == 65534:65534:700 ]] || fail 'Cargo home identity changed'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$WORK_DIR/qualification") == 65534:65534:700 ]] || fail 'metadata qualification directory identity changed'
[[ -z $(/usr/bin/find "$WORK_DIR/cargo-seed" -mindepth 1 -print -quit) ]] || fail 'Cargo home is not empty'
[[ -z $(/usr/bin/find "$WORK_DIR/qualification" -mindepth 1 -print -quit) ]] || fail 'metadata qualification directory is not empty'
[[ -f $HOME_HELPER && ! -L $HOME_HELPER ]] || fail 'managed-home helper is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a:%s' "$HOME_HELPER") == 0:0:444:15680 ]] || fail 'managed-home helper identity changed'
[[ $(/usr/bin/sha256sum "$HOME_HELPER" | /usr/bin/cut -d' ' -f1) == "$HOME_HELPER_SHA256" ]] || fail 'managed-home helper digest mismatch'
[[ -f $WORK_DIR/rustup-home-prepare.py && ! -L $WORK_DIR/rustup-home-prepare.py ]] || fail 'managed-home helper mountpoint is missing'
[[ $(/usr/bin/stat -c '%u:%g:%a:%s' "$WORK_DIR/rustup-home-prepare.py") == 0:0:444:0 ]] || fail 'managed-home helper mountpoint identity changed'

/usr/bin/mount --make-rprivate /
/usr/bin/mount --bind "$ROOTFS" "$ROOTFS"
/usr/bin/mount --bind "$SOURCE_DIR" "$ROOTFS/input"
/usr/bin/mount --bind "$WORK_DIR" "$ROOTFS/work"
/usr/bin/mount --bind "$HOME_HELPER" "$ROOTFS/work/rustup-home-prepare.py"
/usr/bin/mount --bind /etc/resolv.conf "$ROOTFS/etc/resolv.conf"
/usr/bin/mount -t proc -o ro,nosuid,nodev,noexec,hidepid=2,subset=pid proc "$ROOTFS/proc"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev,noexec "$ROOTFS/input"
/usr/bin/mount -o remount,bind,rw,nosuid,nodev,noexec "$ROOTFS/work"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev,noexec "$ROOTFS/work/rustup-home-prepare.py"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev,noexec "$ROOTFS/etc/resolv.conf"
/usr/bin/mount -o remount,ro,nosuid,nodev,noexec,hidepid=2,subset=pid "$ROOTFS/proc"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev "$ROOTFS"

has_mount_option() {
  local path=$1 option=$2 options
  options=$(/usr/bin/findmnt -rn -o OPTIONS --target "$path")
  case ",$options," in
    *",$option,"*) return 0 ;;
    *) return 1 ;;
  esac
}

has_mount_option "$ROOTFS" ro || fail 'rootfs is not read-only'
has_mount_option "$ROOTFS/input" ro || fail 'source is not read-only'
has_mount_option "$ROOTFS/input" noexec || fail 'source permits execution'
has_mount_option "$ROOTFS/work" rw || fail 'task work is not writable'
has_mount_option "$ROOTFS/work" noexec || fail 'task work permits execution'
has_mount_option "$ROOTFS/work/rustup-home-prepare.py" ro || fail 'managed-home helper is not read-only'
has_mount_option "$ROOTFS/work/rustup-home-prepare.py" noexec || fail 'managed-home helper mount permits execution'
has_mount_option "$ROOTFS/etc/resolv.conf" ro || fail 'resolver is not read-only'
has_mount_option "$ROOTFS/proc" ro || fail 'private proc is not read-only'
has_mount_option "$ROOTFS/proc" hidepid=2 || fail 'private proc hidepid policy differs'
has_mount_option "$ROOTFS/proc" subset=pid || fail 'private proc subset policy differs'

# shellcheck disable=SC2016
/usr/sbin/chroot "$ROOTFS" /usr/bin/setpriv \
  --reuid=65534 --regid=65534 --clear-groups --bounding-set=-all \
  --inh-caps=-all --ambient-caps=-all --no-new-privs --pdeathsig=KILL \
  --reset-env -- /usr/bin/env -i \
  PATH=/tools/mise/bin:/tools/bin:/bin:/usr/bin \
  HOME=/work/home USER=builder LOGNAME=builder SHELL=/bin/sh \
  TMPDIR=/work/tmp CARGO_HOME=/work/cargo-seed RUSTUP_HOME=/work/rustup-home \
  RUSTUP_TOOLCHAIN=1.98.1-x86_64-unknown-linux-gnu RUSTUP_AUTO_INSTALL=0 \
  CARGO_TARGET_DIR=/work/target CARGO_BUILD_JOBS=1 \
  CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse CARGO_NET_RETRY=2 \
  CARGO_HTTP_TIMEOUT=60 CARGO_HTTP_LOW_SPEED_LIMIT=1024 CARGO_HTTP_LOW_SPEED_TIMEOUT=30 \
  MISE_CARGO_HOME=/work/cargo-seed MISE_RUSTUP_HOME=/work/rustup-home \
  MISE_DATA_DIR=/work/mise-data MISE_CONFIG_DIR=/tools/mise-config \
  MISE_CACHE_DIR=/work/mise-cache MISE_NO_CONFIG=1 MISE_NO_ENV=1 \
  MISE_NO_HOOKS=1 MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
  MISE_NOT_FOUND_AUTO_INSTALL=0 MISE_OFFLINE=1 \
  XDG_DATA_HOME=/work/xdg-data XDG_CACHE_HOME=/work/xdg-cache \
  XDG_STATE_HOME=/work/xdg-state XDG_CONFIG_HOME=/work/xdg-config \
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 LANG=C LC_ALL=C \
  EXPECTED_MNT_NS="$EXPECTED_MNT_NS" EXPECTED_PID_NS="$EXPECTED_PID_NS" \
  EXPECTED_IPC_NS="$EXPECTED_IPC_NS" EXPECTED_UTS_NS="$EXPECTED_UTS_NS" \
  EXPECTED_NET_NS="$EXPECTED_NET_NS" \
  /bin/sh -ceu '
set -eu
[ "$(/usr/bin/id -u)" = 65534 ]
[ "$(/usr/bin/id -g)" = 65534 ]
[ "$(/usr/bin/id -G)" = 65534 ]
[ "$HOME" = /work/home ]
[ "$CARGO_HOME" = /work/cargo-seed ]
[ "$RUSTUP_HOME" = /work/rustup-home ]
[ "$RUSTUP_TOOLCHAIN:$RUSTUP_AUTO_INSTALL" = 1.98.1-x86_64-unknown-linux-gnu:0 ]
[ "$MISE_NO_CONFIG:$MISE_NO_ENV:$MISE_NO_HOOKS" = 1:1:1 ]
[ "$MISE_AUTO_INSTALL:$MISE_EXEC_AUTO_INSTALL:$MISE_OFFLINE" = false:false:1 ]
[ "$CARGO_REGISTRIES_CRATES_IO_PROTOCOL" = sparse ]
[ -r /input/Cargo.lock ]
[ -r /input/Cargo.toml ]
[ -x /tools/mise/bin/mise ]
[ -x /tools/mise-data/installs/rust/1.98.1/bin/cargo ]
[ -d /work/cargo-seed ]
[ -d /work/rustup-home ]
[ -d /work/mise-data ]
[ -d /work/qualification ]
[ ! -e /work/qualification/cargo-metadata.json ]
[ -z "$(/usr/bin/find /work/rustup-home -mindepth 1 -print -quit)" ]
[ -z "$(/usr/bin/find /work/mise-data -mindepth 1 -print -quit)" ]
for entry in /work/cargo-seed/* /work/cargo-seed/.[!.]* /work/cargo-seed/..?*; do
  [ -e "$entry" ] || continue
  echo "Cargo home is not empty" >&2
  exit 2
done
[ ! -e /work/cargo-seed/config ]
[ ! -e /work/cargo-seed/config.toml ]
[ ! -e /work/cargo-seed/credentials ]
[ ! -e /work/cargo-seed/credentials.toml ]
cap_eff=
cap_prm=
cap_bnd=
cap_amb=
no_new_privs=
while IFS=" " read -r key value rest; do
  case "$key" in
    CapEff:) cap_eff=$value ;;
    CapPrm:) cap_prm=$value ;;
    CapBnd:) cap_bnd=$value ;;
    CapAmb:) cap_amb=$value ;;
    NoNewPrivs:) no_new_privs=$value ;;
  esac
done < /proc/self/status
[ "$cap_eff:$cap_prm:$cap_bnd:$cap_amb:$no_new_privs" = 0000000000000000:0000000000000000:0000000000000000:0000000000000000:1 ]
for fd_path in /proc/self/fd/*; do
  fd=${fd_path##*/}
  case "$fd" in 0|1|2) ;; *) echo "unexpected inherited descriptor" >&2; exit 2 ;; esac
done
[ "$(/usr/bin/sha256sum /work/rustup-home-prepare.py | /usr/bin/cut -d' ' -f1)" = 5876a1cfb5a1c6a43182995af93a31cb8729fa4421e3c5466a1afe8cfb9e3f0f ]
/usr/bin/python3 -I -S /work/rustup-home-prepare.py
rustup_version=$(/tools/rustup-proxy/rustup --version)
printf "%s\n" "$rustup_version"
case "$rustup_version" in "rustup 1.28.2"*) ;; *) echo "unexpected Rustup version" >&2; exit 3 ;; esac
active_toolchain=$(/tools/rustup-proxy/rustup show active-toolchain)
printf "rustup_active_toolchain=%s\n" "$active_toolchain"
case "$active_toolchain" in 1.98.1-x86_64-unknown-linux-gnu*) ;; *) echo "unexpected active Rustup toolchain" >&2; exit 3 ;; esac
rustup_cargo=$(/tools/rustup-proxy/rustup which cargo)
rustup_rustc=$(/tools/rustup-proxy/rustup which rustc)
[ "$rustup_cargo" = /tools/rust-1.98.1/bin/cargo ]
[ "$rustup_rustc" = /tools/rust-1.98.1/bin/rustc ]
cd /work
umask 077
tool_probe=$(/tools/mise/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- /bin/sh -c "cargo_path=\$(command -v cargo); rustc_path=\$(command -v rustc); case \"\$cargo_path\" in /work/cargo-seed/bin/cargo) ;; *) exit 3 ;; esac; case \"\$rustc_path\" in /work/cargo-seed/bin/rustc) ;; *) exit 3 ;; esac; cargo_version=\$(cargo --version); rustc_version=\$(rustc --version); case \"\$cargo_version\" in cargo\\ 1.98.1*) ;; *) exit 3 ;; esac; case \"\$rustc_version\" in rustc\\ 1.98.1*) ;; *) exit 3 ;; esac; printf \"%s\\n\" \"mise rust@1.98.1 selected \$cargo_path (\$cargo_version) and \$rustc_path (\$rustc_version)\"")
printf "%s\\n" "$tool_probe"
/tools/mise/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo fetch --locked --manifest-path /input/Cargo.toml
CARGO_NET_OFFLINE=true /tools/mise/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo metadata --locked --offline --format-version 1 --manifest-path /input/Cargo.toml > /work/qualification/cargo-metadata.json
printf "offline Cargo metadata qualification completed; no compilation command ran\\n"
'

#!/usr/bin/bash
# Mount one reviewed source and task cache before dropping all capabilities.
set -Eeuo pipefail
umask 077

readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly ROOTFS="$TASK_DIR/rootfs"
readonly SOURCE_DIR=/tmp/velnor-g0-provenance-20261005
readonly WORK_DIR="$TASK_DIR/cargo-fetch-bc7-work"
readonly EXPECTED_SOURCE_COMMIT=bc7f784a51233bd29676353392c9fc07c0ad01a7
readonly EXPECTED_SOURCE_TREE=b3939a4d6010f243485fb528f73af74d49b2fb94
readonly EXPECTED_MAIN_PARENT=703d1b84d9f9e0246a52e3989bf3b78a13bbbf05
readonly EXPECTED_LOCK_SHA256=b43ddef9c7a6b7e4c33cb6a8a4dd3659f26a7645a0f0c09782e04b2ee5b898a4
readonly SOURCE_ORIGIN=https://github.com/tailrocks/velnor-new.git
readonly SOURCE_BRANCH=fix/generator-release-provenance

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

git_safe() {
  /usr/bin/env -i PATH=/usr/bin:/bin GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
    GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 \
    "$ROOTFS/tools/bin/git" -c core.fsmonitor=false -c core.hooksPath=/dev/null \
    -c diff.external= -C "$SOURCE_DIR" "$@"
}

[[ -d $SOURCE_DIR && ! -L $SOURCE_DIR ]] || fail 'source is missing or symlinked'
[[ -x $ROOTFS/tools/bin/git && ! -L $ROOTFS/tools/bin/git ]] || fail 'pinned Git binary is unavailable'
[[ $(/usr/bin/sha256sum "$ROOTFS/tools/bin/git" | /usr/bin/cut -d' ' -f1) == 356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60 ]] || fail 'pinned Git binary digest mismatch'
[[ $(git_safe rev-parse HEAD) == "$EXPECTED_SOURCE_COMMIT" ]] || fail 'source commit changed after outer preflight'
[[ $(git_safe rev-parse 'HEAD^{tree}') == "$EXPECTED_SOURCE_TREE" ]] || fail 'source tree changed after outer preflight'
[[ $(git_safe rev-parse 'HEAD^2') == "$EXPECTED_MAIN_PARENT" ]] || fail 'source main parent changed after outer preflight'
[[ $(git_safe branch --show-current) == "$SOURCE_BRANCH" ]] || fail 'source branch changed after outer preflight'
[[ $(git_safe remote get-url origin) == "$SOURCE_ORIGIN" ]] || fail 'source origin changed after outer preflight'
[[ -z $(git_safe status --porcelain=v1 --untracked-files=all) ]] || fail 'source worktree changed after outer preflight'
[[ ! -e $SOURCE_DIR/.cargo && ! -L $SOURCE_DIR/.cargo ]] || fail 'source Cargo configuration appeared after outer preflight'
[[ $(/usr/bin/sha256sum "$SOURCE_DIR/Cargo.lock" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_LOCK_SHA256" ]] || fail 'source lockfile changed after outer preflight'

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 - "$EXPECTED_MNT_NS" "$EXPECTED_PID_NS" "$EXPECTED_IPC_NS" "$EXPECTED_UTS_NS" "$EXPECTED_NET_NS" <<'PY'
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
[[ $(/usr/bin/stat -c '%u:%g:%a' "$WORK_DIR") == 0:0:755 ]] || fail 'private work directory identity changed'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$WORK_DIR/cargo-seed") == 65534:65534:700 ]] || fail 'Cargo home identity changed'
[[ -z $(/usr/bin/find "$WORK_DIR/cargo-seed" -mindepth 1 -print -quit) ]] || fail 'Cargo home is not empty'

/usr/bin/mount --make-rprivate /
/usr/bin/mount --bind "$ROOTFS" "$ROOTFS"
/usr/bin/mount --bind "$SOURCE_DIR" "$ROOTFS/input"
/usr/bin/mount --bind "$WORK_DIR" "$ROOTFS/work"
/usr/bin/mount --bind /etc/resolv.conf "$ROOTFS/etc/resolv.conf"
/usr/bin/mount -t proc -o ro,nosuid,nodev,noexec,hidepid=2,subset=pid proc "$ROOTFS/proc"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev,noexec "$ROOTFS/input"
/usr/bin/mount -o remount,bind,rw,nosuid,nodev,noexec "$ROOTFS/work"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev,noexec "$ROOTFS/etc/resolv.conf"
/usr/bin/mount -o remount,ro,nosuid,nodev,noexec,hidepid=2,subset=pid "$ROOTFS/proc"
/usr/bin/mount -o remount,ro,nosuid,nodev "$ROOTFS"

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
  TMPDIR=/work/tmp CARGO_HOME=/work/cargo-seed RUSTUP_HOME=/work/rustup \
  CARGO_TARGET_DIR=/work/target CARGO_BUILD_JOBS=1 \
  CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse CARGO_NET_RETRY=2 \
  CARGO_HTTP_TIMEOUT=60 CARGO_HTTP_LOW_SPEED_LIMIT=1024 CARGO_HTTP_LOW_SPEED_TIMEOUT=30 \
  MISE_CARGO_HOME=/work/cargo-seed MISE_RUSTUP_HOME=/work/rustup \
  MISE_DATA_DIR=/tools/mise-data MISE_CONFIG_DIR=/tools/mise-config \
  MISE_CACHE_DIR=/work/mise-cache MISE_NO_CONFIG=1 MISE_NO_ENV=1 \
  MISE_NO_HOOKS=1 MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
  MISE_NOT_FOUND_AUTO_INSTALL=0 MISE_OFFLINE=1 \
  XDG_DATA_HOME=/work/xdg-data XDG_CACHE_HOME=/work/xdg-cache \
  XDG_STATE_HOME=/work/xdg-state XDG_CONFIG_HOME=/work/xdg-config \
  GIT_CONFIG_GLOBAL=/tools/gitconfig GIT_CONFIG_NOSYSTEM=1 \
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
[ "$MISE_NO_CONFIG:$MISE_NO_ENV:$MISE_NO_HOOKS" = 1:1:1 ]
[ "$MISE_AUTO_INSTALL:$MISE_EXEC_AUTO_INSTALL:$MISE_OFFLINE" = false:false:1 ]
[ "$CARGO_REGISTRIES_CRATES_IO_PROTOCOL" = sparse ]
[ -r /input/Cargo.lock ]
[ -r /input/Cargo.toml ]
[ -x /tools/mise/bin/mise ]
[ -x /tools/mise-data/installs/rust/1.98.1/bin/cargo ]
[ -d /work/cargo-seed ]
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
cd /work
umask 077
/tools/mise/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo fetch --locked --manifest-path /input/Cargo.toml
'

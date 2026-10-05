#!/usr/bin/env -S -i PATH=/usr/bin:/bin /usr/bin/bash --noprofile --norc
set -Eeuo pipefail
umask 077
readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly ROOTFS="$TASK_DIR/rootfs"
readonly DIAG_WORK="$TASK_DIR/mise-rust-path-probe-work"
readonly EXPECTED_ROOTFS_TREE_SHA256=117843b6eb5d4d06f2309fa205cc51e679445c1dc6f230461e4bd82aa7bf0684
[[ $# == 0 ]]
[[ $(/usr/bin/id -u) == 0 ]]
[[ $(/usr/bin/hostname -s) == bastion ]]
[[ $(/usr/bin/stat -c '%u:%g:%a' "$DIAG_WORK") == 0:0:755 ]]
[[ $(/usr/bin/stat -c '%u:%g:%a' "$DIAG_WORK/cargo-home") == 65534:65534:700 ]]
[[ -d $ROOTFS/work && -d $ROOTFS/proc ]]
/usr/bin/unshare --mount --pid --ipc --uts --net --fork --kill-child=TERM /usr/bin/bash --noprofile --norc -c '''
set -Eeuo pipefail
umask 077
readonly ROOTFS=/root/.velnor-generator-preview-20261005/rootfs
readonly DIAG_WORK=/root/.velnor-generator-preview-20261005/mise-rust-path-probe-work
/usr/bin/mount --make-rprivate /
/usr/bin/mount --bind "$ROOTFS" "$ROOTFS"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev "$ROOTFS"
/usr/bin/mount --bind "$DIAG_WORK" "$ROOTFS/work"
/usr/bin/mount -o remount,bind,rw,nosuid,nodev,noexec "$ROOTFS/work"
/usr/bin/mount -t proc -o ro,nosuid,nodev,noexec,hidepid=2,subset=pid proc "$ROOTFS/proc"
/usr/bin/mount -o remount,ro,nosuid,nodev,noexec,hidepid=2,subset=pid "$ROOTFS/proc"
/usr/bin/findmnt -rn -o OPTIONS --target "$ROOTFS" | /usr/bin/grep -Eq '(^|,)ro(,|$)'
/usr/bin/findmnt -rn -o OPTIONS --target "$ROOTFS/work" | /usr/bin/grep -Eq '(^|,)rw(,|$)'
/usr/bin/findmnt -rn -o OPTIONS --target "$ROOTFS/work" | /usr/bin/grep -Eq '(^|,)noexec(,|$)'
/usr/bin/findmnt -rn -o OPTIONS --target "$ROOTFS/proc" | /usr/bin/grep -Eq '(^|,)ro(,|$)'
/usr/sbin/chroot "$ROOTFS" /usr/bin/setpriv \
  --reuid=65534 --regid=65534 --clear-groups --bounding-set=-all \
  --inh-caps=-all --ambient-caps=-all --no-new-privs --pdeathsig=KILL \
  --reset-env -- /usr/bin/env -i \
  PATH=/tools/mise/bin:/tools/bin:/bin:/usr/bin \
  HOME=/work/home USER=builder LOGNAME=builder SHELL=/bin/sh \
  TMPDIR=/work/tmp CARGO_HOME=/work/cargo-home RUSTUP_HOME=/work/rustup \
  MISE_CARGO_HOME=/work/cargo-home MISE_RUSTUP_HOME=/work/rustup \
  MISE_DATA_DIR=/tools/mise-data MISE_CONFIG_DIR=/tools/mise-config \
  MISE_CACHE_DIR=/work/mise-cache MISE_NO_CONFIG=1 MISE_NO_ENV=1 \
  MISE_NO_HOOKS=1 MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
  MISE_NOT_FOUND_AUTO_INSTALL=0 MISE_OFFLINE=1 \
  XDG_DATA_HOME=/work/xdg-data XDG_CACHE_HOME=/work/xdg-cache \
  XDG_STATE_HOME=/work/xdg-state XDG_CONFIG_HOME=/work/xdg-config \
  LANG=C LC_ALL=C \
  /bin/sh -ceu '''
[ "$(/usr/bin/id -u)" = 65534 ]
[ "$(/usr/bin/id -g)" = 65534 ]
[ "$(/usr/bin/id -G)" = 65534 ]
[ "$HOME:$CARGO_HOME:$RUSTUP_HOME:$MISE_CARGO_HOME:$MISE_RUSTUP_HOME" = /work/home:/work/cargo-home:/work/rustup:/work/cargo-home:/work/rustup ]
[ "$MISE_NO_CONFIG:$MISE_NO_ENV:$MISE_NO_HOOKS:$MISE_OFFLINE" = 1:1:1:1 ]
[ "$MISE_AUTO_INSTALL:$MISE_EXEC_AUTO_INSTALL:$MISE_NOT_FOUND_AUTO_INSTALL" = false:false:0 ]
[ -d "$MISE_DATA_DIR/installs/rust/1.98.1" ]
[ -x "$MISE_DATA_DIR/installs/rust/1.98.1/bin/cargo" ]
[ -x "$MISE_DATA_DIR/installs/rust/1.98.1/bin/rustc" ]
for fd_path in /proc/self/fd/*; do
 fd=${fd_path##*/}
 case "$fd" in 0|1|2) ;; *) echo "unexpected inherited descriptor" >&2; exit 2 ;; esac
done
/tools/mise/bin/mise --version
/tools/mise/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- /bin/sh -ceu '''
cargo_path=$(command -v cargo)
rustc_path=$(command -v rustc)
cargo_real=$(/usr/bin/readlink -f "$cargo_path")
rustc_real=$(/usr/bin/readlink -f "$rustc_path")
cargo_version=$(cargo --version)
rustc_version=$(rustc --version)
[ "$cargo_real" = /tools/rust-1.98.1/bin/cargo ]
[ "$rustc_real" = /tools/rust-1.98.1/bin/rustc ]
case "$cargo_version" in "cargo 1.98.1"*) ;; *) exit 3 ;; esac
case "$rustc_version" in "rustc 1.98.1"*) ;; *) exit 3 ;; esac
printf "cargo_path=%s\ncargo_real=%s\ncargo_version=%s\nrustc_path=%s\nrustc_real=%s\nrustc_version=%s\nRUSTUP_TOOLCHAIN=%s\n" \
  "$cargo_path" "$cargo_real" "$cargo_version" "$rustc_path" "$rustc_real" "$rustc_version" "${RUSTUP_TOOLCHAIN:-unset}"
'''
'''''

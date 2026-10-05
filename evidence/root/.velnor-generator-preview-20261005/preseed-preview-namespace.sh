#!/usr/bin/bash
# Root setup in private namespaces. It runs no repository code as root.
set -Eeuo pipefail
umask 077

[[ $# == 8 ]] || { echo "namespace runner requires eight fixed operands" >&2; exit 2; }
profile=$1
rootfs=$2
source=$3
helper=$4
manifest=$5
inside=$6
work=$7
out=$8

[[ $profile == main || $profile == pr65 ]]
[[ $rootfs == /root/.velnor-generator-preview-20261005/rootfs ]]
[[ $inside == /root/.velnor-generator-preview-20261005/preseed-preview-inside.sh ]]
[[ $work == /root/.velnor-generator-preview-20261005/work/$profile ]]
[[ $out == /root/.velnor-generator-preview-20261005/out/$profile ]]
[[ -d $rootfs && -d $source && -f $helper && -f $manifest && -f $inside && -d $work && -d $out ]]

/usr/bin/mount --make-rprivate /
/usr/bin/mount --bind "$rootfs" "$rootfs"
/usr/bin/mount --bind "$source" "$rootfs/input"
/usr/bin/mount --bind "$helper" "$rootfs/tools/velnor-actions"
/usr/bin/mount --bind "$manifest" "$rootfs/tools/preseed-manifest.json"
/usr/bin/mount --bind "$inside" "$rootfs/tools/run-preview.sh"
/usr/bin/mount --bind "$work" "$rootfs/work"
/usr/bin/mount --bind "$out" "$rootfs/out"
/usr/bin/mount -t proc -o ro,nosuid,nodev,noexec,hidepid=2,subset=pid proc "$rootfs/proc"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev,noexec "$rootfs/input"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev "$rootfs/tools/velnor-actions"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev,noexec "$rootfs/tools/preseed-manifest.json"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev,noexec "$rootfs/tools/run-preview.sh"
/usr/bin/mount -o remount,bind,rw,nosuid,nodev,noexec "$rootfs/work"
/usr/bin/mount -o remount,bind,rw,nosuid,nodev,noexec "$rootfs/out"
/usr/bin/mount -o remount,ro,nosuid,nodev,noexec,hidepid=2,subset=pid "$rootfs/proc"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev "$rootfs"

exec /usr/sbin/chroot "$rootfs" /usr/bin/setpriv \
  --reuid=65534 --regid=65534 --clear-groups --bounding-set=-all \
  --inh-caps=-all --ambient-caps=-all --no-new-privs --pdeathsig=KILL \
  --reset-env -- /usr/bin/env -i \
  HOME=/work/home USER=builder LOGNAME=builder SHELL=/bin/sh PROFILE="$profile" \
  PATH=/tools/mise/bin:/tools/bin:/bin:/usr/bin \
  TMPDIR=/work/tmp CARGO_HOME=/work/empty-cargo RUSTUP_HOME=/work/rustup \
  CARGO_TARGET_DIR=/work/empty-target CARGO_BUILD_JOBS=1 CARGO_NET_OFFLINE=true \
  RUSTUP_TOOLCHAIN=1.98.1 RUSTC=/tools/mise-data/installs/rust/1.98.1/bin/rustc \
  RUSTDOC=/tools/mise-data/installs/rust/1.98.1/bin/rustdoc \
  MISE_CARGO_HOME=/work/empty-cargo MISE_RUSTUP_HOME=/work/rustup \
  MISE_DATA_DIR=/tools/mise-data MISE_CACHE_DIR=/work/empty-mise \
  MISE_CONFIG_DIR=/tools/mise-config MISE_NO_CONFIG=1 MISE_NO_ENV=1 \
  MISE_NO_HOOKS=1 MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
  MISE_NOT_FOUND_AUTO_INSTALL=0 MISE_OFFLINE=1 \
  XDG_DATA_HOME=/work/xdg-data XDG_CACHE_HOME=/work/empty-xdg \
  XDG_STATE_HOME=/work/xdg-state XDG_CONFIG_HOME=/work/xdg-config \
  GIT_CONFIG_GLOBAL=/tools/gitconfig GIT_CONFIG_NOSYSTEM=1 \
  GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 LANG=C LC_ALL=C \
  CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
  /bin/sh /tools/run-preview.sh

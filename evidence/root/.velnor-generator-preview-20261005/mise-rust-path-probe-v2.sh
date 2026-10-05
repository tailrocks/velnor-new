#!/usr/bin/env -S -i PATH=/usr/bin:/bin /usr/bin/bash --noprofile --norc
set -Eeuo pipefail
umask 077
readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly ROOTFS="$TASK_DIR/rootfs"
readonly DIAG_WORK="$TASK_DIR/mise-rust-path-probe-work"
[[ $# == 0 ]]
[[ $(/usr/bin/id -u) == 0 ]]
[[ $(/usr/bin/hostname -s) == bastion ]]
[[ $(/usr/bin/stat -c '%u:%g:%a' "$DIAG_WORK") == 0:0:755 ]]
[[ $(/usr/bin/stat -c '%u:%g:%a' "$DIAG_WORK/cargo-home") == 65534:65534:700 ]]
[[ -d $ROOTFS/work && -d $ROOTFS/proc ]]
/usr/bin/unshare --mount --pid --ipc --uts --net --fork --kill-child=TERM /usr/bin/bash --noprofile --norc -s <<'NAMESPACE'
set -Eeuo pipefail
umask 077
readonly ROOTFS=/root/.velnor-generator-preview-20261005/rootfs
readonly DIAG_WORK=/root/.velnor-generator-preview-20261005/mise-rust-path-probe-work
/usr/bin/mount --make-rprivate /
/usr/bin/mount --bind "$ROOTFS" "$ROOTFS"
/usr/bin/mount --bind "$DIAG_WORK" "$ROOTFS/work"
/usr/bin/mount -t proc -o ro,nosuid,nodev,noexec,hidepid=2,subset=pid proc "$ROOTFS/proc"
/usr/bin/mount -o remount,bind,rw,nosuid,nodev,noexec "$ROOTFS/work"
/usr/bin/mount -o remount,ro,nosuid,nodev,noexec,hidepid=2,subset=pid "$ROOTFS/proc"
/usr/bin/mount -o remount,bind,ro,nosuid,nodev "$ROOTFS"
options=$(/usr/bin/findmnt -rn -o OPTIONS --target "$ROOTFS")
case ",$options," in *",ro,"*) ;; *) exit 20 ;; esac
options=$(/usr/bin/findmnt -rn -o OPTIONS --target "$ROOTFS/work")
case ",$options," in *",rw,"*) ;; *) exit 21 ;; esac
case ",$options," in *",noexec,"*) ;; *) exit 22 ;; esac
options=$(/usr/bin/findmnt -rn -o OPTIONS --target "$ROOTFS/proc")
case ",$options," in *",ro,"*) ;; *) exit 23 ;; esac
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
  LANG=C LC_ALL=C /bin/sh /work/rust-selection-probe.sh
NAMESPACE

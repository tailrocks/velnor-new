#!/bin/sh
# Runs as UID/GID 65534 inside the reviewed task rootfs.
set -eu
[ "$#" -eq 0 ]
case "${PROFILE:-}" in
  main)
    OUT_A=/out/main-a
    OUT_B=/out/main-b
    LOG_A=/out/main-a.log
    LOG_B=/out/main-b.log
    CARGO_A=/work/main-cargo-a
    CARGO_B=/work/main-cargo-b
    TARGET_A=/work/main-target-a
    TARGET_B=/work/main-target-b
    MISE_A=/work/main-mise-a
    MISE_B=/work/main-mise-b
    XDG_A=/work/main-xdg-a
    XDG_B=/work/main-xdg-b
    ;;
  pr65)
    OUT_A=/out/pr65-a
    OUT_B=/out/pr65-b
    LOG_A=/out/pr65-a.log
    LOG_B=/out/pr65-b.log
    CARGO_A=/work/pr65-cargo-a
    CARGO_B=/work/pr65-cargo-b
    TARGET_A=/work/pr65-target-a
    TARGET_B=/work/pr65-target-b
    MISE_A=/work/pr65-mise-a
    MISE_B=/work/pr65-mise-b
    XDG_A=/work/pr65-xdg-a
    XDG_B=/work/pr65-xdg-b
    ;;
  *) echo "unrecognized fixed profile" >&2; exit 2 ;;
esac

[ -e /proc/self/exe ] || { echo "private proc is not mounted" >&2; exit 2; }
proc_mount_ok=false
while IFS=' ' read -r device mountpoint fstype mount_options rest; do
  if [ "$device" = proc ] && [ "$mountpoint" = /proc ] && [ "$fstype" = proc ]; then
    case ",$mount_options," in
      *,ro,* ) case ",$mount_options," in
        *,nosuid,* ) case ",$mount_options," in
          *,nodev,* ) case ",$mount_options," in
            *,noexec,* ) case ",$mount_options," in
              *,hidepid=2,* ) case ",$mount_options," in
                *,subset=pid,* ) proc_mount_ok=true ;;
              esac ;;
            esac ;;
          esac ;;
        esac ;;
      esac ;;
    esac
  fi
done < /proc/self/mounts
[ "$proc_mount_ok" = true ] || { echo "private proc mount options do not match" >&2; exit 2; }
[ ! -e /run/docker.sock ] || { echo "runtime socket is present" >&2; exit 2; }
[ ! -e /var/run/docker.sock ] || { echo "runtime socket is present" >&2; exit 2; }
[ "$HOME" = /work/home ]
[ "$CARGO_NET_OFFLINE" = true ]
[ "$MISE_DATA_DIR" = /tools/mise-data ]
[ "$MISE_NO_CONFIG" = 1 ]
[ "$MISE_NO_ENV" = 1 ]
[ "$MISE_NO_HOOKS" = 1 ]
[ "$MISE_AUTO_INSTALL" = false ]
[ "$MISE_EXEC_AUTO_INSTALL" = false ]
[ "$MISE_OFFLINE" = 1 ]
[ "$GIT_CONFIG_GLOBAL" = /tools/gitconfig ]
[ "$GIT_CONFIG_NOSYSTEM" = 1 ]
[ "$GIT_TERMINAL_PROMPT" = 0 ]
[ -x /tools/velnor-actions ]
[ -x /tools/mise/bin/mise ]
[ -x /tools/bin/git ]
[ -f /tools/preseed-manifest.json ]
[ -d /tools/cargo-seed ]
[ -f /tools/cargo-seed-manifest.json ]

/usr/bin/mkdir -p "$HOME" "$TMPDIR" "$RUSTUP_HOME" /work/empty-mise /work/empty-xdg \
  /work/xdg-data /work/xdg-state /work/xdg-config

/usr/bin/setpriv --dump > "/out/$PROFILE-privilege.txt"
if [ -f /input/.velnor/release-manifest.json ]; then
  printf 'release manifest present\n' > "/out/$PROFILE-release-manifest.txt"
else
  printf 'release manifest absent; private preview only\n' > "/out/$PROFILE-release-manifest.txt"
fi
/tools/mise/bin/mise --version > "/out/$PROFILE-mise-version.txt"
/tools/mise/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- rustc --version > "/out/$PROFILE-rustc-version.txt"
/tools/mise/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo --version > "/out/$PROFILE-cargo-version.txt"
/tools/mise/bin/mise --no-config --no-env --no-hooks exec actionlint@1.7.12 -- actionlint --version > "/out/$PROFILE-actionlint-version.txt"
/tools/mise/bin/mise --no-config --no-env --no-hooks exec shellcheck@0.11.0 -- shellcheck --version > "/out/$PROFILE-shellcheck-version.txt"
/tools/mise/bin/mise --no-config --no-env --no-hooks exec zizmor@1.30.1 -- zizmor --version > "/out/$PROFILE-zizmor-version.txt"
cd /input
mkdir -p "$CARGO_A" "$CARGO_B"
/usr/bin/cp -R /tools/cargo-seed/. "$CARGO_A/"
/usr/bin/cp -R /tools/cargo-seed/. "$CARGO_B/"
export CARGO_HOME="$CARGO_A" MISE_CARGO_HOME="$CARGO_A" CARGO_TARGET_DIR="$TARGET_A"
export MISE_CACHE_DIR="$MISE_A" XDG_CACHE_HOME="$XDG_A"
/tools/velnor-actions generate --mode hosted --output-dir "$OUT_A" > "$LOG_A" 2>&1

export CARGO_HOME="$CARGO_B" MISE_CARGO_HOME="$CARGO_B" CARGO_TARGET_DIR="$TARGET_B"
export MISE_CACHE_DIR="$MISE_B" XDG_CACHE_HOME="$XDG_B"
/tools/velnor-actions generate --mode hosted --output-dir "$OUT_B" > "$LOG_B" 2>&1

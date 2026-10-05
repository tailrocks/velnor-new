#!/bin/sh
set -eu
[ "$(/usr/bin/id -u)" = 65534 ]
[ "$(/usr/bin/id -g)" = 65534 ]
[ "$(/usr/bin/id -G)" = 65534 ]
[ "$HOME:$CARGO_HOME:$RUSTUP_HOME:$MISE_CARGO_HOME:$MISE_RUSTUP_HOME" = /work/home:/work/cargo-home:/work/rustup:/work/cargo-home:/work/rustup ]
[ "$MISE_NO_CONFIG:$MISE_NO_ENV:$MISE_NO_HOOKS:$MISE_OFFLINE" = 1:1:1:1 ]
[ "$MISE_AUTO_INSTALL:$MISE_EXEC_AUTO_INSTALL:$MISE_NOT_FOUND_AUTO_INSTALL" = false:false:0 ]
[ -x "$MISE_DATA_DIR/installs/rust/1.98.1/bin/cargo" ]
[ -x "$MISE_DATA_DIR/installs/rust/1.98.1/bin/rustc" ]
mise_version=$(/tools/mise/bin/mise --version)
case "$mise_version" in "2026.9.18"*) ;; *) echo "unexpected Mise version: $mise_version" >&2; exit 4 ;; esac
printf "mise_version=%s\n" "$mise_version"
probe_status=0
if /tools/mise/bin/mise --no-config --no-env --no-hooks exec rust@1.98.1 -- /bin/sh -c '
  set +e
  cargo_path=$(command -v cargo 2>/dev/null)
  rustc_path=$(command -v rustc 2>/dev/null)
  if [ -n "$cargo_path" ]; then cargo_real=$(/usr/bin/readlink -f "$cargo_path"); else cargo_real=unresolved; fi
  if [ -n "$rustc_path" ]; then rustc_real=$(/usr/bin/readlink -f "$rustc_path"); else rustc_real=unresolved; fi
  cargo --version; cargo_status=$?
  rustc --version; rustc_status=$?
  printf "cargo_path=%s\ncargo_real=%s\ncargo_status=%s\nrustc_path=%s\nrustc_real=%s\nrustc_status=%s\nRUSTUP_TOOLCHAIN=%s\n" \
    "$cargo_path" "$cargo_real" "$cargo_status" "$rustc_path" "$rustc_real" "$rustc_status" "${RUSTUP_TOOLCHAIN:-unset}"
  [ "$cargo_real" = /tools/rust-1.98.1/bin/cargo ] && [ "$rustc_real" = /tools/rust-1.98.1/bin/rustc ] && [ "$cargo_status" = 0 ] && [ "$rustc_status" = 0 ]
'; then
  probe_status=0
else
  probe_status=$?
fi
printf "mise_exec_status=%s\n" "$probe_status"
exit "$probe_status"

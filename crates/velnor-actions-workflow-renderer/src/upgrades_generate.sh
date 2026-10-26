# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# What `velnor-actions generate` needs before it runs, shared by prepare and
# verify: the Velnor-pinned validators it executes (`VELNOR_GENERATE_TOOLS`),
# the project Rust for its offline `cargo metadata`, and every tracked Cargo
# workspace's sources. Installs are fresh (this workflow restores no tool
# cache) and run outside the project configuration (`--no-config`); the
# Rust is verified file by file (`VELNOR_VERIFY_RUST_SCRIPT`) before use.
velnor_generate_prerequisites() {
  local roots rust root
  # shellcheck disable=SC2086 # one exact tool spec per word
  mise --no-config --no-env --no-hooks install $VELNOR_GENERATE_TOOLS || return 1
  roots=$(git ls-files -- Cargo.lock '*/Cargo.lock' | xargs -r -n1 dirname | sort -u)
  [ -n "$roots" ] || return 0
  mise trust --yes . >/dev/null || return 1
  rust=$(mise ls --local --json | jq -r '(.rust // .["core:rust"] // [])[0].requested_version // empty') || return 1
  case "$rust" in
    *[!0-9.]*|'') printf '::error::upgrade_regenerate_rust_unknown: no exact Rust pin (%s)\n' "$rust"; return 1 ;;
  esac
  mise --no-config --no-env --no-hooks install "rust@$rust" || return 1
  # Nothing runs the fresh toolchain before it matches its pinned tarballs.
  VELNOR_TOOL_RUST=$rust sh -c "$VELNOR_VERIFY_RUST_SCRIPT" || return 1
  for root in $roots; do
    mise --no-config --no-env --no-hooks exec "rust@$rust" -- cargo fetch --locked --manifest-path "$root/Cargo.toml" || return 1
  done
}

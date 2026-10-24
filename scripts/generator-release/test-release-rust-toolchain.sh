#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/scripts/generator-release/qualification-goldens.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/velnor-release-rust-test.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/cargo-home" "$WORK/rustup" "$WORK/alias-target" "$WORK/installs"
ln -s "$WORK/alias-target" "$WORK/installs/rust-1.99.1"
MISE_TRACE="$WORK/mise.trace"
RUSTUP_HOME="$WORK/rustup"
TEST_INSTALLED=true

mise() {
  printf '%s\n' "$*" >>"$MISE_TRACE"
  if [ "${MISE_AUTO_INSTALL-}" != false ] || [ "${MISE_EXEC_AUTO_INSTALL-}" != false ] \
    || [ "${CARGO_HOME-}" != "$WORK/cargo-home" ] || [ "${RUSTUP_HOME-}" != "$WORK/rustup" ]; then
    echo "unexpected Mise environment" >&2
    return 90
  fi
  case " $* " in
    *" install "*) echo "unexpected Mise install" >&2; return 91 ;;
  esac
  case "$*" in
    *" ls --json rust")
      printf '[{"version":"1.99.1","installed":%s}]\n' "$TEST_INSTALLED"
      ;;
    *" rust@1.99.1 -- rustc --version")
      printf 'rustc 1.99.1 (test)\n'
      ;;
    *" rust@1.99.1 -- cargo --version")
      printf 'cargo 1.99.1 (test)\n'
      ;;
    *) echo "unexpected Mise argv: $*" >&2; return 92 ;;
  esac
}

alias_identity() {
  python3 - "$WORK/installs/rust-1.99.1" <<'PYTHON'
import os
import stat
import sys

path = sys.argv[1]
info = os.lstat(path)
target = os.readlink(path) if stat.S_ISLNK(info.st_mode) else ""
print(f"{info.st_dev}:{info.st_ino}:{info.st_mode:o}:{target}")
PYTHON
}

before="$(alias_identity)"
release_rust_toolchain_ready 1.99.1 "$WORK/cargo-home" "$WORK/rustup"
if [ "$(wc -l <"$MISE_TRACE" | tr -d ' ')" -ne 3 ] \
  || [ "$(rg -c 'exec rust@1\.99\.1' "$MISE_TRACE")" -ne 2 ]; then
  echo "FAIL: installed toolchain check did not inspect and verify both tools" >&2
  exit 1
fi
TEST_INSTALLED=false
if release_rust_toolchain_ready 1.99.1 "$WORK/cargo-home" "$WORK/rustup" 2>"$WORK/missing.log"; then
  echo "FAIL: missing pinned Rust toolchain was accepted" >&2
  exit 1
fi
if ! grep -Fq 'pinned Rust toolchain rust@1.99.1 is not installed' "$WORK/missing.log"; then
  cat "$WORK/missing.log" >&2
  echo "FAIL: missing toolchain did not produce an explicit diagnostic" >&2
  exit 1
fi
after="$(alias_identity)"
if [ "$before" != "$after" ] \
  || [ "$(wc -l <"$MISE_TRACE" | tr -d ' ')" -ne 4 ] \
  || [ "$(rg -c 'exec rust@1\.99\.1' "$MISE_TRACE")" -ne 2 ]; then
  echo "FAIL: missing pin changed an install alias or tried to execute a tool" >&2
  exit 1
fi
echo "passed installed-pin, disabled-autoinstall, owned-Cargo-home, and no-alias-mutation checks"

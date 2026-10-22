#!/usr/bin/env bash
set -euo pipefail

tar_bin="${TAR_BIN:-tar}"
if ! "$tar_bin" --version | sed -n '1p' | grep -q '^tar (GNU tar) '; then
  echo "GNU tar is required." >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/velnor-gnu-tar-paths.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
export work tar_bin
passed=0
failed=0

run_case() {
  local name="$1" test_function="$2" function_source
  function_source="$(declare -f)"
  if bash -euo pipefail -c "$function_source"$'\n'"$test_function"; then
    passed=$((passed + 1))
    printf 'PASS %s\n' "$name"
  else
    failed=$((failed + 1))
    printf 'FAIL %s\n' "$name" >&2
  fi
}

make_unsafe_paths_archive() {
  python3 - "$work/unsafe.tar" "$work/absolute-target.txt" <<'PY'
import io
import sys
import tarfile

archive, absolute = sys.argv[1:]
with tarfile.open(archive, "w", format=tarfile.PAX_FORMAT) as output:
    for name, content in (("../escape.txt", b"escape"), (absolute, b"absolute")):
        item = tarfile.TarInfo(name)
        item.size = len(content)
        output.addfile(item, io.BytesIO(content))
PY
}

case_default_paths_stay_in_destination() {
  local dest abs_relative status=0
  dest="$work/path-dest"
  abs_relative="${work#/}/absolute-target.txt"
  mkdir -p -- "$dest"
  make_unsafe_paths_archive
  "$tar_bin" -xf "$work/unsafe.tar" -C "$dest" > "$work/unsafe.log" 2>&1 || status=$?
  [ "$status" -eq 2 ]
  [ ! -e "$work/escape.txt" ]
  [ ! -e "$work/absolute-target.txt" ]
  [ ! -e "$dest/escape.txt" ]
  grep -Fq "Member name contains '..'" "$work/unsafe.log"
  grep -qx 'absolute' "$dest/$abs_relative"
}

make_symlink_escape_archive() {
  python3 - "$work/symlink.tar" <<'PY'
import io
import sys
import tarfile

with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as output:
    link = tarfile.TarInfo("link")
    link.type = tarfile.SYMTYPE
    link.linkname = "../outside"
    output.addfile(link)
    item = tarfile.TarInfo("link/payload")
    item.size = 5
    output.addfile(item, io.BytesIO(b"pwned"))
    safe = tarfile.TarInfo("safe.txt")
    safe.size = 4
    output.addfile(safe, io.BytesIO(b"safe"))
PY
}

case_archive_symlink_cannot_escape() {
  local dest status=0
  dest="$work/symlink-dest"
  mkdir -p -- "$dest" "$work/outside"
  make_symlink_escape_archive
  "$tar_bin" -xf "$work/symlink.tar" -C "$dest" > "$work/symlink.log" 2>&1 || status=$?
  [ "$status" -le 2 ]
  [ ! -e "$work/outside/payload" ]
  grep -qx 'safe' "$dest/safe.txt"
}

case_preexisting_symlink_cannot_escape() {
  local dest status=0
  dest="$work/preexisting-dest"
  mkdir -p -- "$dest" "$work/outside"
  ln -s ../outside "$dest/link"
  python3 - "$work/preexisting.tar" <<'PY'
import io
import sys
import tarfile

with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as output:
    item = tarfile.TarInfo("link/payload")
    item.size = 5
    output.addfile(item, io.BytesIO(b"pwned"))
PY
  "$tar_bin" -xf "$work/preexisting.tar" -C "$dest" > "$work/preexisting.log" 2>&1 || status=$?
  [ "$status" -le 2 ]
  [ ! -e "$work/outside/payload" ]
}

case_internal_symlink_works() {
  local src="$work/internal-src" dest="$work/internal-dest"
  mkdir -p -- "$src" "$dest"
  printf 'target-ok\n' > "$src/target"
  ln -s target "$src/link"
  "$tar_bin" -cf "$work/internal.tar" -C "$src" target link
  "$tar_bin" -xf "$work/internal.tar" -C "$dest"
  [ "$(readlink "$dest/link")" = target ]
  grep -qx 'target-ok' "$dest/link"
}

run_case default-parent-and-absolute-paths-stay-in-destination case_default_paths_stay_in_destination
run_case archive-symlink-cannot-escape case_archive_symlink_cannot_escape
run_case preexisting-symlink-cannot-escape case_preexisting_symlink_cannot_escape
run_case internal-symlink-works case_internal_symlink_works
printf 'RESULT pass=%s fail=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]

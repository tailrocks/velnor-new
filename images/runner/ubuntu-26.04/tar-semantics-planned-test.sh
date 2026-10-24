#!/usr/bin/env bash
set -euo pipefail

tar_bin="${TAR_BIN:-/usr/bin/tar.gnu}"
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

case_in_root_symlink_with_p_restores_child() {
  local dest="$work/in-root-p-dest"
  mkdir -p -- "$dest"
  python3 - "$work/in-root-p.tar" <<'PY'
import io
import sys
import tarfile

with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as output:
    target = tarfile.TarInfo("target")
    target.type = tarfile.DIRTYPE
    target.mode = 0o755
    output.addfile(target)
    parent = tarfile.TarInfo("dir")
    parent.type = tarfile.DIRTYPE
    parent.mode = 0o755
    output.addfile(parent)
    link = tarfile.TarInfo("dir/link")
    link.type = tarfile.SYMTYPE
    link.linkname = "../target"
    output.addfile(link)
    child = tarfile.TarInfo("dir/link/payload")
    child.size = 5
    output.addfile(child, io.BytesIO(b"safe\n"))
PY
  "$tar_bin" -xPf "$work/in-root-p.tar" -C "$dest"
  [ -L "$dest/dir/link" ]
  [ "$(readlink "$dest/dir/link")" = ../target ]
  grep -qx 'safe' "$dest/target/payload"
  grep -qx 'safe' "$dest/dir/link/payload"
}

case_external_cache_relative_symlink_with_p_is_preserved() {
  local root="$work/external-cache-p" dest link
  root="$work/external-cache-p"
  dest="$root/dest/workspace/repo"
  link="$root/dest/workspace/external/cache/link"
  mkdir -p -- "$dest" "$(dirname -- "$link")"
  python3 - "$root/cache-link.tar" <<'PY'
import sys
import tarfile

with tarfile.open(sys.argv[1], "w", format=tarfile.PAX_FORMAT) as output:
    item = tarfile.TarInfo("../external/cache/link")
    item.type = tarfile.SYMTYPE
    item.linkname = "../../shared/cache-target"
    output.addfile(item)
PY
  "$tar_bin" -xPf "$root/cache-link.tar" -C "$dest"
  [ -L "$link" ]
  [ "$(readlink "$link")" = ../../shared/cache-target ]
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
run_case in-root-symlink-with-p-restores-child case_in_root_symlink_with_p_restores_child
run_case external-cache-relative-symlink-with-p-is-preserved case_external_cache_relative_symlink_with_p_is_preserved
run_case internal-symlink-works case_internal_symlink_works
printf 'RESULT pass=%s fail=%s\n' "$passed" "$failed"
[ "$failed" -eq 0 ]

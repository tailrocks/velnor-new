#!/usr/bin/env bash

archive_guard_source_fingerprint() {
  local fingerprint
  fingerprint="$(python3 -I -B -c '
import os
from pathlib import Path
import stat
import sys

repository = Path(sys.argv[1])
directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
file_flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
root_fd = os.open(repository, directory_flags)
scripts_fd = os.open("scripts", directory_flags, dir_fd=root_fd)
descriptor = os.open("owned_archive_preflight.py", file_flags, dir_fd=scripts_fd)
try:
    before = os.fstat(descriptor)
    if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1
            or before.st_uid != os.getuid() or before.st_mode & 0o022
            or before.st_size > 16 * 1024 * 1024):
        raise ValueError("archive preflight source has untrusted provenance")
    with os.fdopen(descriptor, "rb", closefd=False) as stream:
        source = stream.read(16 * 1024 * 1024 + 1)
    after = os.fstat(descriptor)
    named = os.stat("owned_archive_preflight.py", dir_fd=scripts_fd,
                    follow_symlinks=False)
    before_identity = (before.st_dev, before.st_ino, before.st_size,
                       before.st_mtime_ns, before.st_ctime_ns, before.st_mode,
                       before.st_nlink, before.st_uid)
    after_identity = (after.st_dev, after.st_ino, after.st_size,
                      after.st_mtime_ns, after.st_ctime_ns, after.st_mode,
                      after.st_nlink, after.st_uid)
    if (len(source) > 16 * 1024 * 1024 or before_identity != after_identity
            or (before.st_dev, before.st_ino) != (named.st_dev, named.st_ino)):
        raise ValueError("archive preflight source changed during verification")
finally:
    os.close(descriptor)
    os.close(scripts_fd)
    os.close(root_fd)

namespace = {"__file__": str(repository / "scripts" / "owned_archive_preflight.py"),
             "__name__": "owned_archive_preflight"}
exec(compile(source, namespace["__file__"], "exec"), namespace)
print(namespace["_source_fingerprint"](repository))
' "$repository")" || fail 'cannot compute admitted archive guard source fingerprint'
  [[ "$fingerprint" =~ ^[0-9a-f]{64}$ ]] \
    || fail 'archive guard source fingerprint is malformed'
  printf '%s' "$fingerprint"
}

archive_guard_target_identity() {
  local rustc_verbose rustc_details rustc_release rustc_commit rustc_host llvm_version
  local cargo_identity platform_name platform_machine
  rustc_verbose="$(run_mise exec rust@1.98.1 -- rustc -vV)" \
    || fail 'cannot inspect the pinned Rust compiler'
  rustc_details="$(awk -F ': ' '
    $1 == "release" { release = $2; release_count++ }
    $1 == "commit-hash" { commit = $2; commit_count++ }
    $1 == "host" { host = $2; host_count++ }
    $1 == "LLVM version" { llvm = $2; llvm_count++ }
    END {
      if (release_count != 1 || commit_count != 1 || host_count != 1 || llvm_count != 1) {
        exit 1
      }
      printf "%s\t%s\t%s\t%s\n", release, commit, host, llvm
    }
  ' <<<"$rustc_verbose")" || fail 'cannot identify the pinned Rust compiler'
  IFS=$'\t' read -r rustc_release rustc_commit rustc_host llvm_version \
    <<<"$rustc_details"
  cargo_identity="${cargo_version#cargo }"
  cargo_identity="${cargo_identity// /-}"
  cargo_identity="${cargo_identity//(/}"
  cargo_identity="${cargo_identity//)/}"
  platform_name="$(uname -s)"
  platform_machine="$(uname -m)"
  [[ "$rustc_release" == 1.98.1 && "$rustc_commit" =~ ^[A-Za-z0-9._-]+$ \
    && "$rustc_host" =~ ^[A-Za-z0-9._-]+$ && "$llvm_version" =~ ^[A-Za-z0-9._-]+$ \
    && "$cargo_identity" =~ ^[A-Za-z0-9._-]+$ \
    && "$platform_name" =~ ^[A-Za-z0-9._-]+$ \
    && "$platform_machine" =~ ^[A-Za-z0-9._-]+$ ]] \
    || fail 'Rust, Cargo, or platform identity is malformed'
  printf '%s-%s-%s-rustc-%s-%s-llvm-%s-cargo-%s' \
    "$platform_name" "$platform_machine" "$rustc_host" "$rustc_release" \
    "$rustc_commit" "$llvm_version" "$cargo_identity"
}

archive_guard_check_fingerprint() {
  local executable="$1" expected="$2"
  if ! "$executable" --fingerprint >"$run_tmp/fingerprint.stdout" \
    2>"$run_tmp/fingerprint.stderr"; then
    fail 'archive guard fingerprint command failed'
  fi
  printf '%s\n' "$expected" >"$run_tmp/fingerprint.expected"
  cmp -s "$run_tmp/fingerprint.stdout" "$run_tmp/fingerprint.expected" \
    && [[ ! -s "$run_tmp/fingerprint.stderr" ]] \
    || fail 'compiled archive guard fingerprint differs from admitted source'
}

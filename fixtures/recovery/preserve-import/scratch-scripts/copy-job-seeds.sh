#!/usr/bin/env bash
# Copy staged mbx/<job-id> trees into a seed root.
# The seed root keeps generator, mise, and rustup.
# This script does not start Docker.
set -euo pipefail

die() {
  printf '%s\n' "$1" >&2
  exit 1
}

main() {
  src="${1:-}"
  dest_root="${2:-}"
  shift 2 || die "usage: copy-job-seeds.sh SRC DEST_ROOT JOB..."
  [ -n "$src" ] || die "source is empty"
  [ -n "$dest_root" ] || die "dest root is empty"
  if [ -L "$src" ] || [ ! -d "$src" ]; then
    die "source is not a directory"
  fi
  if [ -L "$dest_root" ] || [ ! -d "$dest_root" ]; then
    die "dest root is not a directory"
  fi
  if [ "$#" -eq 0 ]; then
    die "job list is empty"
  fi
  mkdir -p "$dest_root/mbx"
  if [ -L "$dest_root/mbx" ]; then
    die "dest mbx is a symlink"
  fi
  for job in "$@"; do
    case "$job" in
      "" | *[!A-Za-z0-9_.-]*) die "job id is unsafe" ;;
    esac
    if [ -L "$src/$job" ] || [ ! -d "$src/$job" ]; then
      die "staged job $job is not a directory"
    fi
    if [ -L "$src/$job/PREFIX" ] || [ ! -f "$src/$job/PREFIX" ]; then
      die "staged PREFIX for $job is missing"
    fi
    if [ -L "$src/$job/bundle" ] || [ ! -d "$src/$job/bundle" ]; then
      die "staged bundle for $job is missing"
    fi
    prefix="$(awk 'NR==1 { print; exit }' "$src/$job/PREFIX")"
    case "$prefix" in
      *"-${job}-") ;;
      *) die "PREFIX does not name $job" ;;
    esac
    target="$dest_root/mbx/$job"
    if [ -L "$target" ]; then
      die "dest job $job is a symlink"
    fi
    rm -rf "$target"
    mkdir -p "$target"
    cp -a "$src/$job/PREFIX" "$target/PREFIX"
    cp -a "$src/$job/bundle" "$target/bundle"
    [ -f "$target/PREFIX" ] || die "copied PREFIX for $job is missing"
    [ -d "$target/bundle" ] || die "copied bundle for $job is missing"
  done
  printf '%s\n' "job seeds copied"
}

main "$@"

#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -eu
[ "$1" = "--no-config" ] && [ "$2" = "--no-env" ] && [ "$3" = "--no-hooks" ] || exit 90
shift 3
if [ "$1" = "ls" ]; then
  [ "$2" = "--json" ] || exit 91
  case "$3" in
    rust) printf '[{"version":"@RUST_VERSION@","installed":true}]\n' ;;
    mr-boxington) printf '[{"version":"@MBX_VERSION@","installed":true}]\n' ;;
    *) exit 92 ;;
  esac
  exit 0
fi
[ "$1" = "exec" ] || exit 93
shift
if [ "$1" = "@JACTIONLINT_SPEC@" ]; then
  shift
  [ "$1" = "@SHELLCHECK_SPEC@" ] || exit 94
  shift
  [ "$1" = "--" ] && [ "$2" = "jactionlint" ] || exit 95
  shift 2
  [ "$1" = "--config-file" ] && [ "$2" = ".github/jactionlint.yaml" ] || exit 96
  [ "$3" = "--format" ] && [ "$4" = "oneline" ] || exit 97
  shift 4
  for flag in --strict-exit --no-online --shellcheck=shellcheck --pyflakes= --no-color; do
    [ "$1" = "$flag" ] || exit 98
    shift
  done
  [ "$#" -gt 0 ] || exit 99
  for file in "$@"; do [ -f "$file" ] || exit 100; done
  exit 0
fi
if [ "$1" = "@SHELLCHECK_SPEC@" ]; then
  shift
  [ "$1" = "--" ] && [ "$2" = "shellcheck" ] || exit 101
  shift 2
  if [ "$1" = "--version" ]; then
    [ "$#" -eq 1 ] || exit 102
    printf 'version: @SHELLCHECK_VERSION@\n'
    exit 0
  fi
  if [ "$1" = "-S" ]; then
    [ "$2" = "warning" ] && [ "$3" = "--format=gcc" ] || exit 103
    shift 3
  else
    [ "$1" = "--format=gcc" ] && [ "$2" = "--include=SC2086" ] || exit 104
    shift 2
  fi
  [ "$#" -gt 0 ] || exit 105
  for file in "$@"; do [ -f "$file" ] || exit 106; done
  exit 0
fi
[ "$1" = "@RUST_SPEC@" ] || exit 94
shift
if [ "$1" = "@MBX_SPEC@" ]; then shift; fi
[ "$1" = "--" ] || exit 95
shift
if [ "$1" = "cargo" ]; then
  shift
  [ "$1" = "metadata" ] || exit 96
  shift
elif [ "$1" = "mbx" ]; then
  shift
  [ "$1" = "metadata" ] || exit 97
  shift
else
  exit 98
fi
[ "$1" = "--format-version" ] && [ "$2" = "1" ] || exit 99
shift 2
if [ "$1" = "--no-deps" ]; then
  shift
elif [ "$1" = "--locked" ]; then
  shift
  [ "$1" = "--offline" ] || exit 100
  shift
  [ "$1" = "--all-features" ] || exit 101
  shift
else
  exit 102
fi
[ "$1" = "--manifest-path" ] || exit 103
[ "$2" = "@MANIFEST_PATH@" ] || exit 104
if [ "${VELNOR_TEST_GUARD_REQUIRED:-0}" = "1" ] && [ "${VELNOR_TEST_GUARD_ADMITTED:-0}" != "1" ]; then
  exit 105
fi
cat "$(dirname "$0")/metadata.json"

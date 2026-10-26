#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

source "$ROOT/scripts/generator-release/fixture-git.sh"

test_fixture_git_isolation() (
  local base="$WORK/git-isolation" repo marker config original pointer
  mkdir -p "$base/repo" "$base/attacker"
  repo="$base/repo"; marker="$base/hook-ran"; config="$base/global.gitconfig"
  export VELNOR_FIXTURE_POISON_MARKER="$marker"
  command git config --file "$config" hook.fixture-poison.event pre-commit
  command git config --file "$config" hook.fixture-poison.command 'printf poisoned >> "$VELNOR_FIXTURE_POISON_MARKER"; exit 73'
  export GIT_CONFIG_GLOBAL="$config" GIT_CONFIG_SYSTEM="$config"
  export GIT_CONFIG_COUNT=2 GIT_CONFIG_KEY_0=hook.fixture-poison.event
  export GIT_CONFIG_VALUE_0=pre-commit GIT_CONFIG_KEY_1=hook.fixture-poison.command
  export GIT_CONFIG_VALUE_1='printf poisoned >> "$VELNOR_FIXTURE_POISON_MARKER"; exit 73'
  fixture_git -C "$repo" init -q || return 1
  printf 'fixture LFS source bytes\n' > "$repo/marker.txt"
  printf 'marker.txt filter=lfs diff=lfs merge=lfs -text\n' > "$repo/.gitattributes"
  fixture_git -C "$repo" add -A || return 1
  fixture_git -C "$repo" commit -qm fixture || return 1
  [ ! -e "$marker" ] || { echo 'FAIL: fixture executed ambient native hook' >&2; return 1; }
  pointer=$(fixture_git -C "$repo" show HEAD:marker.txt) || return 1
  printf '%s\n' "$pointer" | grep -Fqx 'version https://git-lfs.github.com/spec/v1' || return 1
  printf '%s\n' "$pointer" | grep -Fqx "oid sha256:$(test_file_sha256 "$repo/marker.txt")" || return 1
  fixture_git -C "$base/attacker" init -q || return 1
  original=$(test_file_sha256 "$base/attacker/.git/config") || return 1
  export GIT_DIR="$base/attacker/.git" GIT_WORK_TREE="$base/attacker"
  export GIT_INDEX_FILE="$base/attacker/redirected-index" GIT_COMMON_DIR="$base/attacker/.git"
  export GIT_OBJECT_DIRECTORY="$base/attacker/objects" GIT_ALTERNATE_OBJECT_DIRECTORIES="$base/attacker/objects"
  export GIT_CONFIG_PARAMETERS="'core.hooksPath'='$base/attacker'" GIT_TEMPLATE_DIR="$base/attacker"
  printf 'second source\n' > "$repo/second.txt"
  fixture_git -C "$repo" add second.txt || return 1
  fixture_git -C "$repo" commit -qm second || return 1
  [ "$(fixture_git -C "$repo" rev-list --count HEAD)" = 2 ] || return 1
  [ "$original" = "$(test_file_sha256 "$base/attacker/.git/config")" ] || return 1
  [ ! -e "$base/attacker/redirected-index" ] && [ ! -e "$marker" ] || return 1
  echo 'passed fixture Git authority isolation and LFS pointer semantics'
)

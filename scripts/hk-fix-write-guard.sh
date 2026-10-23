#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
  printf '%s\n' 'usage: hk-fix-write-guard.sh COMMAND [ARG ...]' >&2
  exit 64
fi

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

if command -v shasum >/dev/null 2>&1; then
  hash_stream() {
    local digest
    digest=$(shasum -a 256 | awk '{print $1}') || return $?
    [[ "$digest" =~ ^[[:xdigit:]]{64}$ ]] || return 70
    printf '%s\n' "$digest"
  }
elif command -v sha256sum >/dev/null 2>&1; then
  hash_stream() {
    local digest
    digest=$(sha256sum | awk '{print $1}') || return $?
    [[ "$digest" =~ ^[[:xdigit:]]{64}$ ]] || return 70
    printf '%s\n' "$digest"
  }
else
  printf '%s\n' 'Cargo fix checks require a SHA-256 tool.' >&2
  exit 69
fi

untracked_hash() {
  local untracked path digest mode
  untracked=$(git ls-files --others --exclude-standard -z |
    while IFS= read -r -d '' path; do
      if [[ -L "$path" ]]; then
        digest=$(readlink "./$path" | hash_stream) || exit $?
        printf 'link\0%s\0%s\0' "$path" "$digest"
      elif [[ -f "$path" ]]; then
        digest=$(hash_stream <"$path") || exit $?
        if [[ -x "$path" ]]; then
          mode=executable
        else
          mode=regular
        fi
        printf 'file\0%s\0%s\0%s\0' "$path" "$mode" "$digest"
      else
        printf 'missing\0%s\0\0' "$path"
      fi
    done | hash_stream) || return $?
  printf '%s\n' "$untracked"
}

snapshot() {
  local worktree untracked status_hash index
  worktree=$(git diff --no-ext-diff --binary -- | hash_stream) || return $?
  untracked=$(untracked_hash) || return $?
  status_hash=$(git status --porcelain=v1 --untracked-files=all -z | hash_stream) || return $?
  index=$(git ls-files --stage -z | hash_stream) || return $?
  printf '%s:%s:%s:%s\n' "$worktree" "$untracked" "$status_hash" "$index"
}

before_status=0
before=$(snapshot) || before_status=$?
if [[ $before_status -ne 0 ]]; then
  printf '%s\n' 'The script cannot inspect Git state before the Cargo command.' >&2
  exit "$before_status"
fi
command_status=0
"$@" || command_status=$?
after_status=0
after=$(snapshot) || after_status=$?

if [[ $command_status -ne 0 ]]; then
  exit "$command_status"
fi
if [[ $after_status -ne 0 ]]; then
  printf '%s\n' 'The script cannot inspect Git state after the Cargo command.' >&2
  exit 70
fi

if [[ "$before" != "$after" ]]; then
  printf '%s\n' 'A Cargo fix changed repository files or the Git index. Review the changes and run verification again before you commit.' >&2
  exit 1
fi

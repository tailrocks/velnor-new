#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
: "${GITHUB_WORKSPACE:?}" "${RUNNER_TEMP:?}" "${CARGO_TARGET_DIR:?}"
if ! [[ "$GITHUB_WORKSPACE" == /* && "$RUNNER_TEMP" == /* && "$CARGO_TARGET_DIR" == "$RUNNER_TEMP/velnor-ci-image-target" && ! -L "$CARGO_TARGET_DIR" ]]; then
  printf '%s\n' 'invalid private CARGO_TARGET_DIR' >&2
  exit 1
fi
workspace_dir="$(cd -P -- "$GITHUB_WORKSPACE" && pwd -P)"
temp_dir="$(cd -P -- "$RUNNER_TEMP" && pwd -P)"
if ! [[ "$temp_dir" != "$workspace_dir" && "$temp_dir" != "$workspace_dir/"* ]]; then
  printf '%s\n' 'invalid private CARGO_TARGET_DIR' >&2
  exit 1
fi
trusted_git() { /usr/bin/env -i PATH=/usr/bin:/bin GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_OPTIONAL_LOCKS=0 /usr/bin/git --git-dir="$GITHUB_WORKSPACE/.git" --work-tree="$GITHUB_WORKSPACE" -c core.fsmonitor=false "$@"; }
source_sha="$(trusted_git rev-parse --verify HEAD)"
test "$source_sha" = "$GITHUB_SHA"
if [ "$GITHUB_EVENT_NAME" != pull_request ]; then test "$GITHUB_WORKFLOW_SHA" = "$GITHUB_SHA"; fi
if ! trusted_git diff --cached --quiet --no-ext-diff --no-textconv --ignore-submodules=none || ! trusted_git diff --quiet --no-ext-diff --no-textconv --ignore-submodules=none; then
  printf '%s\n' 'The source checkout is not clean.' >&2
  exit 1
fi
status_file="$(mktemp "$RUNNER_TEMP/velnor-image-source-status.XXXXXXXX")"
trusted_git status --porcelain=v1 --untracked-files=all --ignore-submodules=none -z > "$status_file"
if [[ -s "$status_file" ]]; then
  printf '%s\n' 'The source checkout is not clean.' >&2
  exit 1
fi
metadata_file="$(mktemp "$RUNNER_TEMP/velnor-native-image-metadata.XXXXXXXX")"
artifacts_file="$(mktemp "$RUNNER_TEMP/velnor-native-image-artifacts.XXXXXXXX")"
trap 'rm -f "$metadata_file" "$artifacts_file" "$status_file"' EXIT
cargo metadata --locked --format-version=1 --filter-platform x86_64-unknown-linux-gnu > "$metadata_file"
"$@" --message-format=json > "$artifacts_file"
python3 scripts/native-image-candidate-inventory.py --check --root "$GITHUB_WORKSPACE" --metadata "$metadata_file" --artifacts "$artifacts_file" --target x86_64-unknown-linux-gnu
test -f "$CARGO_TARGET_DIR/release/velnor-actions" && test ! -L "$CARGO_TARGET_DIR/release/velnor-actions"
umask 077
image_binary_dir="$(mktemp -d "$RUNNER_TEMP/velnor-native-image-actions.XXXXXXXX")"
image_binary="$image_binary_dir/velnor-native-image-actions"
( set -o noclobber; cat "$CARGO_TARGET_DIR/release/velnor-actions" > "$image_binary" )
chmod 0500 "$image_binary"
printf 'VELNOR_NATIVE_IMAGE_ACTIONS=%s\n' "$image_binary" >> "$GITHUB_ENV"

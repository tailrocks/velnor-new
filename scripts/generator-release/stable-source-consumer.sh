#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN GH_TOKEN MISE_GITHUB_TOKEN GH_HOST GH_CONFIG_DIR CARGO_BUILD_TARGET SOURCES_CACHE_HIT SOURCES_CACHE_MATCHED_KEY
[[ "$RUNNER_TEMP" == /* && "$(cd "$RUNNER_TEMP" && pwd -P)" == "$RUNNER_TEMP" ]]
mkdir -p "$RUNNER_TEMP/velnor"
[[ ! -e "$RUNNER_TEMP/velnor/cargo" && ! -L "$RUNNER_TEMP/velnor/cargo" ]]
mkdir -m 700 "$RUNNER_TEMP/velnor/cargo"
export CARGO_HOME="$RUNNER_TEMP/velnor/cargo" VELNOR_PREFETCH_RUST_ONLY=1
stage="$RUNNER_TEMP/stable-cargo-download"
admitted=0
prepare_stage() {
  mkdir -p "$stage/registry/index" "$stage/registry/cache" || return 1
  if [[ -d "$stage/git/db" && ! -L "$stage/git/db" ]]; then
    for db in "$stage"/git/db/*; do
      [[ -e "$db" ]] || continue
      [[ -d "$db" && ! -L "$db" && "${db##*/}" =~ ^[^/]+-[0-9a-f]{16}$ ]] || return 1
      mkdir -p "$db/objects/info" "$db/objects/pack" || return 1
    done
  fi
  bash scripts/generator-release/cargo-source-handoff.sh copy "$stage" "$CARGO_HOME"
}
if [[ "${SOURCES_DOWNLOADED:-}" == success && -d "$stage" && ! -L "$stage" ]] &&
  [[ "$(/usr/bin/python3 -I -S scripts/generator-release/cargo-source-tree.py "$stage")" == "$SOURCES_TREE" ]] &&
  prepare_stage; then
  admitted=1
else
  rm -rf -- "$CARGO_HOME"
  mkdir -m 700 "$CARGO_HOME"
fi
rm -rf -- "$stage"
mkdir -p "$RUNNER_TEMP/source-qualification-artifacts/logs"
export VELNOR_SOURCE_ARTIFACT_ADMITTED="$admitted"
bash scripts/generator-release/source-qualification.sh prefetch-locked-sources-for-qualification-workspaces
for manifest in Cargo.toml crates/velnor-runner/Cargo.toml; do
  mise --no-config --no-env --no-hooks exec "rust@$VELNOR_RUST_VERSION" -- cargo metadata --locked --offline --all-features --format-version=1 --manifest-path "$GITHUB_WORKSPACE/$manifest" > /dev/null
done
printf 'CARGO_HOME=%s\n' "$CARGO_HOME" >> "$GITHUB_ENV"

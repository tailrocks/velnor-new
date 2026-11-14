#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN GH_TOKEN MISE_GITHUB_TOKEN GH_HOST GH_CONFIG_DIR CARGO_BUILD_TARGET SOURCES_CACHE_HIT SOURCES_CACHE_MATCHED_KEY
[[ "$RUNNER_TEMP" == /* && "$(cd "$RUNNER_TEMP" && pwd -P)" == "$RUNNER_TEMP" ]]
[[ "$(git rev-parse HEAD)" == "$GITHUB_SHA" && "$GITHUB_WORKFLOW_SHA" == "$GITHUB_SHA" ]]
mkdir -p "$RUNNER_TEMP/velnor"
[[ ! -e "$RUNNER_TEMP/velnor/cargo" && ! -L "$RUNNER_TEMP/velnor/cargo" ]]
mkdir -m 700 "$RUNNER_TEMP/velnor/cargo" "$RUNNER_TEMP/stable-cargo-sources"
mkdir -p "$RUNNER_TEMP/source-qualification-artifacts/logs"
export CARGO_HOME="$RUNNER_TEMP/velnor/cargo" VELNOR_PREFETCH_RUST_ONLY=1
bash scripts/generator-release/source-qualification.sh prefetch-locked-sources-for-qualification-workspaces
bash scripts/generator-release/cargo-source-handoff.sh copy "$CARGO_HOME" "$RUNNER_TEMP/stable-cargo-sources"
tree="$(/usr/bin/python3 -I -S scripts/generator-release/cargo-source-tree.py "$RUNNER_TEMP/stable-cargo-sources")"
root_lock="$(shasum -a 256 Cargo.lock | cut -d ' ' -f 1)"
runner_lock="$(shasum -a 256 crates/velnor-runner/Cargo.lock | cut -d ' ' -f 1)"
printf 'CARGO_HOME=%s\nSTABLE_SOURCES_TREE=%s\nSTABLE_SOURCES_ROOT_LOCK=%s\nSTABLE_SOURCES_RUNNER_LOCK=%s\nSTABLE_SOURCES_ATTEMPT=%s\n' "$CARGO_HOME" "$tree" "$root_lock" "$runner_lock" "$GITHUB_RUN_ATTEMPT" >> "$GITHUB_ENV"

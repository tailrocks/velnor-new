#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
umask 077
export RUSTUP_TOOLCHAIN="${VELNOR_EXPECTED_RUSTUP_TOOLCHAIN:?}"
export MBX_SHARE_OUT_DIR=0
root="$(git rev-parse --show-toplevel)"
source_sha="$(git rev-parse --verify 'HEAD^{commit}')"
test "$source_sha" = "${GITHUB_SHA:?}"
test -z "$(git status --porcelain=v1 --untracked-files=all)"
work="$(mktemp -d "${RUNNER_TEMP:?}/runner-candidate.XXXXXXXX")"
export CARGO_TARGET_DIR="$work/target"
rustup target list --installed | grep -Fx x86_64-unknown-linux-musl >/dev/null
cargo metadata --locked --format-version 1 --filter-platform x86_64-unknown-linux-gnu > "$work/root-metadata.json"
cargo build --locked -p velnor-actions-cli --bin velnor-actions -p velnor-actions-contract --example runner_family_inputs --message-format=json-render-diagnostics > "$work/root-artifacts.jsonl"
(
  for name in ${!GIT_@}; do unset "$name"; done
  export GIT_NO_REPLACE_OBJECTS=1 GIT_NO_LAZY_FETCH=1 GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0
  /usr/bin/git -C "$root" cat-file blob "$source_sha:.velnor/runner.lock" > "$work/committed-runner.lock"
)
/usr/bin/cmp -s "$root/.velnor/runner.lock" "$work/committed-runner.lock"
CARGO_TARGET_DIR="$work/qualifier-target" cargo metadata --manifest-path crates/velnor-runner/Cargo.toml --locked --format-version 1 --filter-platform x86_64-unknown-linux-gnu > "$work/host-metadata.json"
env -u VELNOR_SOURCE_SHA -u VELNOR_RUNNER_BUILD_SOURCE_SHA -u VELNOR_RUNNER_BUILD_HELPER_SHA256 CARGO_TARGET_DIR="$work/qualifier-target" cargo build --manifest-path crates/velnor-runner/Cargo.toml --locked -p velnor-runner-host --bin velnor-resource-probe-release --message-format=json-render-diagnostics > "$work/host-artifacts.jsonl"
cargo metadata --manifest-path crates/velnor-runner/Cargo.toml --locked --format-version 1 --filter-platform x86_64-unknown-linux-musl > "$work/metadata.json"
VELNOR_SOURCE_SHA="$source_sha" cargo build --manifest-path crates/velnor-runner/Cargo.toml --locked --release --target x86_64-unknown-linux-musl --message-format=json-render-diagnostics -p velnor-resource-probe --bin velnor-resource-probe --bin velnor-storage-init --bin velnor-job-start-hook > "$work/artifacts.jsonl"
test "$source_sha" = "$(git rev-parse --verify 'HEAD^{commit}')"
test -z "$(git status --porcelain=v1 --untracked-files=all)"
python3 scripts/runner-family-source-inventory.py --check --root "$root" --metadata "$work/metadata.json" --artifacts "$work/artifacts.jsonl" --root-metadata "$work/root-metadata.json" --root-artifacts "$work/root-artifacts.jsonl" --host-metadata "$work/host-metadata.json" --host-artifacts "$work/host-artifacts.jsonl" --target x86_64-unknown-linux-musl --source-commit "$source_sha"
"$CARGO_TARGET_DIR/debug/velnor-actions" image-maintenance install-buildx --root "$root" --runner-temp "$RUNNER_TEMP"
mkdir -m 0700 "$work/contexts"
install -m 0555 "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/velnor-resource-probe" "$work/contexts/resource_probe"
install -m 0555 "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/velnor-storage-init" "$work/contexts/storage_init"
install -m 0555 "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/velnor-job-start-hook" "$work/contexts/job_start_hook"
printf '#!/bin/sh\nset -eu\nexec /velnor/job-start-hook "$@"\n' > "$work/contexts/job_started_hook"
chmod 0555 "$work/contexts/job_started_hook"
mkdir -m 0700 "$RUNNER_TEMP/runner-image-assets-parent"
assets="$RUNNER_TEMP/runner-image-assets-parent/assets"
publication=()
if [[ "${GITHUB_EVENT_NAME:-}" == push && "${GITHUB_REF:-}" == refs/heads/main ]]; then
  publication=(--publication-records)
fi
"$CARGO_TARGET_DIR/debug/velnor-actions" image-maintenance build-runner-images --root "$root" --source-commit "$source_sha" --runner-temp "$RUNNER_TEMP" --contexts-dir "$work/contexts" --output-dir "$assets" "${publication[@]}"
"$work/qualifier-target/debug/velnor-resource-probe-release" check-runner-image-candidate "$root" "$source_sha" "$assets/velnor-runner-linux-amd64.tar" "$assets/velnor-dind-linux-amd64.tar" "$assets/velnor-resource-probe-linux-amd64.tar" "$assets/runner-image-provenance.json" "$assets/dind-image-provenance.json" "$assets/resource-probe-image-provenance.json" "$assets" /var/run/docker.sock

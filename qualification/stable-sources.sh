#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Qualification mode `stable-sources`: the producer, warm and cold jobs of
# qualification.yml run `bash qualification/stable-sources.sh <phase>` for
# every step that the old engine wrote as an inline script. velnor-gen
# refuses a multi-line `run`, a `shell` key and a job `env`, so each script
# moved here, one phase per step. A phase keeps the commands and the order of
# the engine step it replaces.
#
# Phases, in job order:
#   verify-source        the checked-out commit is the workflow commit
#   resolve-tools        VELNOR_TOOL_* from the project's tool files (scrubbed shell)
#   install-tools        mise install of the pinned tools (needs GITHUB_TOKEN)
#   verify-rust          the Rust toolchain against its channel manifest (scrubbed shell)
#   prepare-producer     producer: fetch and copy the Cargo sources
#   record-identity      producer: check the tree and the locks, write the job outputs
#   fetch-metadata       warm: artifact metadata (needs GH_TOKEN, SOURCES_ID)
#   admit                warm: admit the same-run artifact
#   validate-id          warm: the artifact ID is a safe integer
#   measure-warm         warm: source preparation from the downloaded artifact
#   measure-cold         cold: source preparation from the network
#   provenance-warm      warm: the evidence names the artifact and the offline fetch
#   provenance-cold      cold: the evidence names no artifact and the network fetch
#   archive-guard        warm and cold: the archive guard builds offline
#
# What the step list of the engine did that a script cannot:
#   - `shell: bash` is the `set -euo pipefail` below; the shell that starts this
#     file is `bash -e {0}` (the runner default), so BASH_ENV, if an earlier
#     step set it, was read before line 1. The two phases that hold the tool
#     checks start a clean shell (`harden`), as the engine's wrapper did.
#   - `env.VELNOR_TOOL_RUST` reaches a script as $VELNOR_TOOL_RUST, which the
#     resolve-tools phase wrote to $GITHUB_ENV.
#   - `runner.temp` is $RUNNER_TEMP.

set -euo pipefail

phase="${1:?usage: stable-sources.sh <phase>}"
self="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"

# The credentials a step of this flow must not hand to a script it runs.
scrub=(
  -u ACTIONS_ID_TOKEN_REQUEST_TOKEN
  -u ACTIONS_ID_TOKEN_REQUEST_URL
  -u ACTIONS_RUNTIME_TOKEN
  -u GITHUB_TOKEN
  -u MISE_GITHUB_TOKEN
  -u GH_TOKEN
  -u GH_HOST
  -u GH_CONFIG_DIR
)

# Runs <file> in a clean shell: no exported function, no BASH_ENV or ENV, no
# credential, a fixed PATH, bash in privileged mode. This is the wrapper the
# engine put around these two steps; <file> is the body it passed to `bash -c`.
harden() {
  local file="$1" entry scanned=0
  local -a functions=()
  while IFS= read -r -d '' entry; do
    case "$entry" in
      BASH_FUNC_*=*) functions+=(-u "${entry%%=*}") ;;
      VELNOR_VERIFIER_ENV_SCAN_COMPLETE) scanned=$((scanned + 1)) ;;
    esac
  done < <({ /usr/bin/env -0 && printf '%s\000' 'VELNOR_VERIFIER_ENV_SCAN_COMPLETE'; })
  [ "$scanned" -eq 1 ] || exit 1
  exec /usr/bin/env ${functions[@]+"${functions[@]}"} -u BASH_ENV -u ENV "${scrub[@]}" \
    PATH=/usr/bin:/bin:/usr/sbin:/sbin \
    /bin/bash --noprofile --norc -p -e -o pipefail "$file"
}

verify_source() {
  test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
  test "$GITHUB_WORKFLOW_SHA" = "$GITHUB_SHA"
}

install_tools() {
  mise --no-config --no-env --no-hooks install \
    'rust@'"${VELNOR_TOOL_RUST}" 'bun@'"${VELNOR_TOOL_BUN}" \
    'jactionlint@'"${VELNOR_TOOL_JACTIONLINT}" 'shellcheck@'"${VELNOR_TOOL_SHELLCHECK}"
}

prepare_producer() {
  VELNOR_RUST_VERSION="$VELNOR_TOOL_RUST" \
    env "${scrub[@]}" bash scripts/generator-release/stable-source-producer.sh
}

# The identity checks of the producer. It also names the artifact: the
# engine wrote `proof-linux-sources-${{ github.run_attempt }}` and
# `${{ runner.temp }}/stable-cargo-sources/` in the upload step, and velnor-gen
# reads neither context in `with`, so the upload step reads them from here.
record_identity() {
  test "$(env "${scrub[@]}" /usr/bin/python3 -I -S scripts/generator-release/cargo-source-tree.py "$RUNNER_TEMP/stable-cargo-sources")" = "$STABLE_SOURCES_TREE"
  test "$(shasum -a 256 Cargo.lock | cut -d ' ' -f 1)" = "$STABLE_SOURCES_ROOT_LOCK"
  test "$(shasum -a 256 Cargo.lock | cut -d ' ' -f 1)" = "$STABLE_SOURCES_RUNNER_LOCK"
  printf 'tree=%s\nroot_lock=%s\nrunner_lock=%s\nattempt=%s\n' "$STABLE_SOURCES_TREE" "$STABLE_SOURCES_ROOT_LOCK" "$STABLE_SOURCES_RUNNER_LOCK" "$STABLE_SOURCES_ATTEMPT" >> "$GITHUB_OUTPUT"
  printf 'upload_name=proof-linux-sources-%s\nupload_path=%s/stable-cargo-sources/\n' "$GITHUB_RUN_ATTEMPT" "$RUNNER_TEMP" >> "$GITHUB_OUTPUT"
}

fetch_metadata() {
  SOURCES_METADATA="$RUNNER_TEMP/stable-source-metadata.json"
  gh() {
    (
      mise --no-config --no-env --no-hooks exec gh@2.104.0 -- gh "$@" & _velnor_gh_pid=$!
      ( sleep 60; kill -TERM "$_velnor_gh_pid" 2>/dev/null; sleep 5; kill -KILL "$_velnor_gh_pid" 2>/dev/null ) </dev/null >/dev/null 2>&1 & _velnor_gh_watch=$!
      _velnor_gh_status=0
      wait "$_velnor_gh_pid" || _velnor_gh_status=$?
      kill -KILL "$_velnor_gh_watch" 2>/dev/null || true
      wait "$_velnor_gh_watch" 2>/dev/null || true
      exit "$_velnor_gh_status"
    ) </dev/null
  }
  export -f gh
  rm -f -- "$SOURCES_METADATA" "$SOURCES_METADATA.partial"
  trap 'rm -f -- "$SOURCES_METADATA.partial"' EXIT
  [[ "$SOURCES_ID" =~ ^[1-9][0-9]*$ ]]
  gh api --hostname github.com --method GET "repos/tailrocks/velnor-new/actions/artifacts/$SOURCES_ID" | head -c 1048577 > "$SOURCES_METADATA.partial"
  test "$(wc -c < "$SOURCES_METADATA.partial")" -le 1048576
  mv -- "$SOURCES_METADATA.partial" "$SOURCES_METADATA"
}

admit() {
  SOURCES_METADATA="$RUNNER_TEMP/stable-source-metadata.json" \
    env "${scrub[@]}" /usr/bin/python3 -I -S scripts/generator-release/stable-source-admission.py
}

# Also names the download directory for the download step, which cannot read
# `runner.temp` (see record_identity).
validate_id() {
  local length
  [[ "$VELNOR_EVIDENCE_ARTIFACT_ID" =~ ^[1-9][0-9]*$ ]] || exit 1
  length="${#VELNOR_EVIDENCE_ARTIFACT_ID}"
  [ "$length" -le 16 ] || exit 1
  if [ "$length" -eq 16 ]; then [ "$VELNOR_EVIDENCE_ARTIFACT_ID" -le 9007199254740991 ] || exit 1; fi
  printf 'download_path=%s/stable-cargo-download\n' "$RUNNER_TEMP" >> "$GITHUB_OUTPUT"
}

# $1 is what the download step reported. The warm job runs this step only when
# the download step succeeded (no `if` on it, so a failure stops the job
# before), and the cold job has no download step.
measure() {
  VELNOR_RUST_VERSION="$VELNOR_TOOL_RUST" SOURCES_DOWNLOADED="$1" SOURCES_TREE="$2" \
    env "${scrub[@]}" bash scripts/generator-release/stable-source-consumer.sh
}

provenance() {
  local admitted="$1" mode="$2" evidence
  evidence="$RUNNER_TEMP/source-qualification-artifacts/cargo-fetch-lock-identity.txt"
  grep -Fx "source_artifact_admitted=$admitted" "$evidence"
  grep -Fx "fetch_mode=Cargo.toml:$mode" "$evidence"
  grep -Fx "fetch_mode=Cargo.toml:$mode" "$evidence"
}

archive_guard() {
  env "${scrub[@]}" env CARGO_NET_OFFLINE=true bash scripts/build-owned-archive-guard.sh --source-cache "$CARGO_HOME"
}

case "$phase" in
  verify-source) verify_source ;;
  resolve-tools) harden "$self/stable-sources-resolve-tools.sh" ;;
  verify-rust) harden "$self/stable-sources-verify-rust.sh" ;;
  install-tools) install_tools ;;
  prepare-producer) prepare_producer ;;
  record-identity) record_identity ;;
  fetch-metadata) fetch_metadata ;;
  admit) admit ;;
  validate-id) validate_id ;;
  measure-warm) measure success "$SOURCES_TREE" ;;
  measure-cold) measure skipped "" ;;
  provenance-warm) provenance 1 offline ;;
  provenance-cold) provenance 0 network ;;
  archive-guard) archive_guard ;;
  *)
    echo "stable-sources.sh: unknown phase $phase" >&2
    exit 2
    ;;
esac

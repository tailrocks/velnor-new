#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -eu
bin=$1
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
stage="$work/stage"
matrix=m-df42bdfcb20b0e49
run_dir="$stage/runner_temp/velnor/r4242-a1"
report="$run_dir/$matrix/tasks/task-r4242-a1-m-df42bdfcb20b0e49-4fcc45b68b3849cf.json"

mask() {
  sed -E 's/"(started_at|finished_at)":"[^"]*"/"\1":"<clock>"/g'
}

prepare() {
  rm -rf "$stage"
  mkdir -p "$stage/runner_temp" "$stage/checkout/.github/velnor"
  cp -R "$here/emit/velnor" "$stage/runner_temp/velnor"
  cp "$here/emit/task-execution-manifest-v1.json" "$stage/checkout/.github/velnor/task-execution-manifest-v1.json"
  cp "$here/event.json" "$stage/event.json"
}

invoke() {
  op=$1
  status=$2
  shift 2
  (
    cd "$stage/checkout"
    env -i \
      VELNOR_INTERNAL_OP="$op" \
      VELNOR_JOB_STATUS="$status" \
      VELNOR_MATRIX_KEY="$matrix" \
      GITHUB_RUN_ID=4242 \
      GITHUB_RUN_ATTEMPT=1 \
      GITHUB_REPOSITORY=tailrocks/velnor-new \
      GITHUB_JOB=t54-group \
      GITHUB_EVENT_NAME=pull_request \
      GITHUB_EVENT_PATH="$stage/event.json" \
      GITHUB_SHA=0123456789abcdef0123456789abcdef01234567 \
      GITHUB_REF=refs/pull/1/merge \
      GITHUB_WORKFLOW_SHA=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa \
      "$@" \
      "$bin"
  )
}

rm -rf "$here/assemble"
mkdir -p "$here/assemble/reports/aaa-artifact" "$here/assemble/reports/zzz-artifact" "$here/golden"

prepare
invoke emit-group-ledger-v1 success RUNNER_TEMP="$stage/runner_temp" >/dev/null
mask <"$run_dir/$matrix/group-ledger.json" >"$here/golden/emit_success.json"
cp "$run_dir/$matrix/group-ledger.json" "$here/assemble/reports/zzz-artifact/group-ledger.json"

prepare
cp "$here/emit-other-plan.json" "$run_dir/plan.json"
invoke emit-group-ledger-v1 success RUNNER_TEMP="$stage/runner_temp" GITHUB_JOB=t54-other-group >/dev/null
cp "$run_dir/$matrix/group-ledger.json" "$here/assemble/reports/aaa-artifact/group-ledger.json"

prepare
rm "$report"
invoke emit-group-ledger-v1 failure RUNNER_TEMP="$stage/runner_temp" >/dev/null
mask <"$run_dir/$matrix/group-ledger.json" >"$here/golden/emit_needs_prepare.json"

prepare
rm "$report"
invoke emit-group-ledger-v1 success RUNNER_TEMP="$stage/runner_temp" 2>"$here/golden/emit_missing_report.stderr" >/dev/null || true

prepare
cp -R "$here/assemble/reports" "$run_dir/reports"
invoke assemble-group-ledgers-v1 success RUNNER_TEMP="$stage/runner_temp" >/dev/null
cp "$run_dir/group-ledgers.json" "$here/golden/group_ledgers.json"

prepare
mkdir -p "$run_dir/reports/bad-artifact"
printf 'not a ledger' >"$run_dir/reports/bad-artifact/group-ledger.json"
invoke assemble-group-ledgers-v1 success RUNNER_TEMP="$stage/runner_temp" 2>"$here/golden/assemble_corrupt.stderr" >/dev/null || true


#!/bin/sh
# Records what the legacy write-request-v1 op writes for each case here.
# Usage: record.sh <path to the velnor-actions binary built from this repository>
# For each case it writes `expected` (the request file) or `expected_error` (the code the op reports).
set -eu
engine=$1
here=$(cd "$(dirname "$0")" && pwd)
for dir in "$here"/*/; do
  name=$(basename "$dir")
  temp=$(mktemp -d)
  rm -f "$dir/expected" "$dir/expected_error"
  get() { sed -n "s/^$1=//p" "$dir/context"; }
  has() { grep -q "^$1=" "$dir/context"; }
  set -- env -i PATH=/usr/bin:/bin RUNNER_TEMP="$temp" \
    VELNOR_INTERNAL_OP=write-request-v1 \
    VELNOR_REQUEST_FILE="$temp/velnor/request/plan-v1-request.json" \
    GITHUB_EVENT_NAME="$(cat "$dir/event_name")" GITHUB_EVENT_PATH="$dir/payload.json"
  for pair in sha:GITHUB_SHA repository:GITHUB_REPOSITORY ref:GITHUB_REF ref_protected:GITHUB_REF_PROTECTED \
    workflow_ref:GITHUB_WORKFLOW_REF workflow_sha:GITHUB_WORKFLOW_SHA run_id:GITHUB_RUN_ID run_attempt:GITHUB_RUN_ATTEMPT; do
    key=${pair%%:*}
    if has "$key"; then set -- "$@" "${pair#*:}=$(get "$key")"; fi
  done
  [ -f "$dir/named_check_lanes.json" ] && set -- "$@" "VELNOR_NAMED_CHECK_LANES_JSON=$(cat "$dir/named_check_lanes.json")"
  [ -f "$dir/lane_inventory.json" ] && set -- "$@" "VELNOR_LANE_INVENTORY_JSON=$(cat "$dir/lane_inventory.json")"
  if "$@" "$engine" 2> "$temp/stderr"; then
    cp "$temp/velnor/request/plan-v1-request.json" "$dir/expected"
  else
    sed -n 's/^.*internal request failed: internal: //p' "$temp/stderr" | tr -d '\n' > "$dir/expected_error"
  fi
  rm -rf "$temp"
done

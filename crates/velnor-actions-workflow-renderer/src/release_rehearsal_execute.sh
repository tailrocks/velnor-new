#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
unset GITHUB_TOKEN GH_TOKEN ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN
[[ "$VELNOR_SOURCE_SHA256" =~ ^[a-f0-9]{64}$ && "$CANDIDATE_PR" =~ ^[1-9][0-9]*$ && "$CANDIDATE_SHA" =~ ^[a-f0-9]{40}$ ]]
test "$(wc -c < "$RUNNER_TEMP/rehearsal/rehearsal-source.tar")" -le 104857600
printf '%s  %s\n' "$VELNOR_SOURCE_SHA256" "$RUNNER_TEMP/rehearsal/rehearsal-source.tar" | sha256sum -c -
python3 - "$RUNNER_TEMP/rehearsal/rehearsal-source.tar" "$RUNNER_TEMP/source" <<'PYTHON'
__VELNOR_EXTRACT_PYTHON__
PYTHON
current="$RUNNER_TEMP/source"
IFS=/ read -r -a parts <<< "$VELNOR_REHEARSAL_SCRIPT"
for part in "${parts[@]}"; do
  [[ -n "$part" && "$part" != . && "$part" != .. ]]
  current="$current/$part"
  test ! -L "$current"
done
test -f "$current"
cd "$RUNNER_TEMP/source"
exec env -i PATH=/usr/bin:/bin HOME="$RUNNER_TEMP" TMPDIR="$RUNNER_TEMP" RUNNER_TEMP="$RUNNER_TEMP" CANDIDATE_PR="$CANDIDATE_PR" CANDIDATE_SHA="$CANDIDATE_SHA" SOURCE_SHA="$GITHUB_SHA" bash "$current"

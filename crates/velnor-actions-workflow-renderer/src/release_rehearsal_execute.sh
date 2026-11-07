#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
while IFS='=' read -r variable _; do
  if [[ "$variable" == GIT_* ]]; then unset "$variable"; fi
done < <(env)
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0
unset GITHUB_TOKEN GH_TOKEN ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN
[[ "$VELNOR_SOURCE_SHA256" =~ ^[a-f0-9]{64}$ && "$CANDIDATE_PR" =~ ^[1-9][0-9]*$ && "$CANDIDATE_SHA" =~ ^[a-f0-9]{40}$ ]]
test "$(wc -c < "$RUNNER_TEMP/rehearsal/rehearsal-source.tar")" -le 104857600
printf '%s  %s\n' "$VELNOR_SOURCE_SHA256" "$RUNNER_TEMP/rehearsal/rehearsal-source.tar" | sha256sum -c -
python3 - "$RUNNER_TEMP/rehearsal/rehearsal-source.tar" "$RUNNER_TEMP/archive-source" <<'PYTHON'
__VELNOR_EXTRACT_PYTHON__
PYTHON
[[ "$VELNOR_BUNDLE_SHA256" =~ ^[a-f0-9]{64}$ && "$GITHUB_SHA" =~ ^[a-f0-9]{40}$ ]]
test "$(wc -c < "$RUNNER_TEMP/rehearsal/rehearsal-source.bundle")" -le 104857600
printf '%s  %s\n' "$VELNOR_BUNDLE_SHA256" "$RUNNER_TEMP/rehearsal/rehearsal-source.bundle" | sha256sum -c -
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0
mkdir -m 700 "$RUNNER_TEMP/empty-hooks"
git -c core.hooksPath="$RUNNER_TEMP/empty-hooks" clone --no-checkout "$RUNNER_TEMP/rehearsal/rehearsal-source.bundle" "$RUNNER_TEMP/source"
git -C "$RUNNER_TEMP/source" -c core.hooksPath="$RUNNER_TEMP/empty-hooks" checkout --detach "$GITHUB_SHA"
test "$(git -C "$RUNNER_TEMP/source" rev-parse HEAD)" = "$GITHUB_SHA"
source_status="$(git -C "$RUNNER_TEMP/source" status --porcelain --untracked-files=all --ignored=matching)"
test -z "$source_status"
python3 - "$RUNNER_TEMP/source" "$RUNNER_TEMP/recreated.tar" <<'PYTHON'
__VELNOR_ARCHIVE_PYTHON__
PYTHON
cmp "$RUNNER_TEMP/recreated.tar" "$RUNNER_TEMP/rehearsal/rehearsal-source.tar"
current="$RUNNER_TEMP/source"
IFS=/ read -r -a parts <<< "$VELNOR_REHEARSAL_SCRIPT"
for part in "${parts[@]}"; do
  [[ -n "$part" && "$part" != . && "$part" != .. ]]
  current="$current/$part"
  test ! -L "$current"
done
test -f "$current"
cd "$RUNNER_TEMP/source"
exec env -i PATH=/usr/bin:/bin HOME="$RUNNER_TEMP" TMPDIR="$RUNNER_TEMP" RUNNER_TEMP="$RUNNER_TEMP" CANDIDATE_PR="$CANDIDATE_PR" CANDIDATE_SHA="$CANDIDATE_SHA" SOURCE_SHA="$GITHUB_SHA" __VELNOR_OUTPUT_ENV__ bash "$current"

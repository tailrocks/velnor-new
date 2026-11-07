#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
while IFS='=' read -r variable _; do
  if [[ "$variable" == GIT_* ]]; then unset "$variable"; fi
done < <(env)
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0
[[ "$GITHUB_SHA" =~ ^[a-f0-9]{40}$ && "$VELNOR_REHEARSAL_SCRIPT" =~ ^[A-Za-z0-9_./-]+$ ]]
[[ "$CANDIDATE_PR" =~ ^[1-9][0-9]*$ && "$CANDIDATE_SHA" =~ ^[a-f0-9]{40}$ ]]
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_OPTIONAL_LOCKS=0
test "$(git -C policy rev-parse HEAD)" = "$GITHUB_SHA"
test "$(git -C policy write-tree)" = "$(git -C policy rev-parse "$GITHUB_SHA^{tree}")"
source_status="$(git -C policy status --porcelain --untracked-files=all --ignored=matching)"
test -z "$source_status"
mode="$(git -C policy ls-tree "$GITHUB_SHA" -- "$VELNOR_REHEARSAL_SCRIPT" | cut -d' ' -f1)"
[[ "$mode" = 100644 || "$mode" = 100755 ]]
python3 - policy "$RUNNER_TEMP/rehearsal-source.tar" <<'PYTHON'
__VELNOR_ARCHIVE_PYTHON__
PYTHON
test "$(wc -c < "$RUNNER_TEMP/rehearsal-source.tar")" -le 104857600
printf 'source_sha256=%s\n' "$(sha256sum "$RUNNER_TEMP/rehearsal-source.tar" | cut -d' ' -f1)" >> "$GITHUB_OUTPUT"

git -C policy bundle create "$RUNNER_TEMP/rehearsal-source.bundle" HEAD
test "$(wc -c < "$RUNNER_TEMP/rehearsal-source.bundle")" -le 104857600
printf 'bundle_sha256=%s\n' "$(sha256sum "$RUNNER_TEMP/rehearsal-source.bundle" | cut -d' ' -f1)" >> "$GITHUB_OUTPUT"

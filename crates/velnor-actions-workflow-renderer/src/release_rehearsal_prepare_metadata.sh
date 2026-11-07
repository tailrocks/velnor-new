#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
[[ "$SOURCE_SHA" =~ ^[a-f0-9]{40}$ ]]
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_OPTIONAL_LOCKS=0
test "$(git -C approved-source rev-parse HEAD)" = "$SOURCE_SHA"
test "$(git -C approved-source write-tree)" = "$(git -C approved-source rev-parse "$SOURCE_SHA^{tree}")"
printf 'approved_tree=%s\n' "$(git -C approved-source rev-parse "$SOURCE_SHA^{tree}")" >> "$GITHUB_OUTPUT"
test -z "$(git -C approved-source status --porcelain --untracked-files=all)"
python3 - approved-source "$RUNNER_TEMP/approved-source.tar" <<'PYTHON'
__VELNOR_ARCHIVE_PYTHON__
PYTHON
test "$(wc -c < "$RUNNER_TEMP/approved-source.tar")" -le 104857600
printf 'approved_sha256=%s\n' "$(sha256sum "$RUNNER_TEMP/approved-source.tar" | cut -d' ' -f1)" >> "$GITHUB_OUTPUT"

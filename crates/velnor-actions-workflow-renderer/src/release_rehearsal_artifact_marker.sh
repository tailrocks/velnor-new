#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
[[ "$VELNOR_ARTIFACT_ID" =~ ^[1-9][0-9]*$ && "$VELNOR_ARTIFACT_DIGEST" =~ ^[a-f0-9]{64}$ ]]
[[ "$GITHUB_RUN_ID" =~ ^[1-9][0-9]*$ && "$GITHUB_RUN_ATTEMPT" = 1 ]]
printf 'VELNOR_REHEARSAL_WITNESS run=%s attempt=%s artifact=%s sha256=%s\n' \
  "$GITHUB_RUN_ID" "$GITHUB_RUN_ATTEMPT" "$VELNOR_ARTIFACT_ID" "$VELNOR_ARTIFACT_DIGEST"

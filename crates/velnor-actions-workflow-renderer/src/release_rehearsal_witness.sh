#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail
[[ "$GITHUB_SHA" =~ ^[a-f0-9]{40}$ && "$GITHUB_RUN_ID" =~ ^[1-9][0-9]*$ && "$GITHUB_RUN_ATTEMPT" = 1 ]]
[[ "$VELNOR_SOURCE_SHA256" =~ ^[a-f0-9]{64}$ && "$CANDIDATE_PR" =~ ^[1-9][0-9]*$ && "$CANDIDATE_SHA" =~ ^[a-f0-9]{40}$ ]]
jq -n \
  --arg repository "$GITHUB_REPOSITORY" --arg workflow_sha "$GITHUB_SHA" \
  --arg run_id "$GITHUB_RUN_ID" --arg run_attempt "$GITHUB_RUN_ATTEMPT" \
  --arg script "$VELNOR_REHEARSAL_SCRIPT" --arg source_sha256 "$VELNOR_SOURCE_SHA256" \
  --arg environment "$VELNOR_REHEARSAL_ENVIRONMENT" --arg reviewer_id "$VELNOR_REVIEWER_ID" \
  --arg candidate_pr "$CANDIDATE_PR" --arg candidate_sha "$CANDIDATE_SHA" \
  '{schema: 1, observation: "script-exited-successfully", repository: $repository,
    workflow_sha: $workflow_sha, run_id: $run_id, run_attempt: $run_attempt,
    script: $script, source_sha256: $source_sha256, environment: $environment, reviewer_id: $reviewer_id,
    unverified_candidate_pr: $candidate_pr, unverified_candidate_sha: $candidate_sha}' \
  > "$RUNNER_TEMP/rehearsal-observation.json"

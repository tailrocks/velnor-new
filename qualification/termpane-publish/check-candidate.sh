#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
candidate=${CANDIDATE_SHA:?}
number=${PR_NUMBER:?}
[[ "$candidate" =~ ^[0-9a-f]{40}$ ]]
[[ "$number" =~ ^[1-9][0-9]*$ ]]
test "$number" -le 2147483647
fixture="$GITHUB_WORKSPACE/qualification/termpane-publish"
dir=$(mktemp -d "$RUNNER_TEMP/termpane-admission.XXXXXXXX")
"$RUNNER_TEMP/qualification-gh/gh_2.103.3_linux_amd64/bin/gh" api graphql -f query="$(cat "$fixture/pr-query.graphql")" -F number="$number" > "$dir/pr.json"
"$RUNNER_TEMP/qualification-gh/gh_2.103.3_linux_amd64/bin/gh" api repos/tailrocks/velnor-new/rules/branches/main > "$dir/rules.json"
python3 "$fixture/validate-pr.py" "$candidate" "$number" "$dir/pr.json" "$dir/rules.json"

#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

cd "$(dirname "$0")/.."

case "${GITHUB_EVENT_NAME:-}" in
  pull_request | merge_group) ;;
  *)
    echo "helper gates: nothing to compare on a ${GITHUB_EVENT_NAME:-local} run; the merge queue checks every landing"
    exit 0
    ;;
esac

base_ref="$(python3 -I -c '
import json, os
event = json.load(open(os.environ["GITHUB_EVENT_PATH"]))
ref = event["merge_group"]["base_ref"] if "merge_group" in event else event["pull_request"]["base"]["ref"]
print(ref.removeprefix("refs/heads/"))
')"

if [ "$(git rev-parse --is-shallow-repository)" = true ]; then
  git fetch --force --tags --quiet --unshallow origin
else
  git fetch --force --tags --quiet origin
fi
git fetch --force --quiet origin "$base_ref:refs/remotes/origin/$base_ref"

bash scripts/test-check-helper-gates.sh
bash scripts/test-helper-gates-merge-queue.sh
git fetch --force --tags --quiet origin
bash scripts/check-helper-gates.sh "origin/$base_ref"

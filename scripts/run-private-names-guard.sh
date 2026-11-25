#!/usr/bin/env bash

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

guard=scripts/private-names-guard.py
hashes=scripts/private-names-hashes.txt
trusted="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/private-names-policy.XXXXXX")"
trap 'rm -rf "$trusted"' EXIT

cd "$(git rev-parse --show-toplevel)"

base=""
if [ "${GITHUB_EVENT_NAME:-}" = pull_request ]; then
  base="$(python3 -I -c 'import json, os; print(json.load(open(os.environ["GITHUB_EVENT_PATH"]))["pull_request"]["base"]["sha"])')"
  git cat-file -e "$base^{commit}" 2>/dev/null || git fetch --quiet --no-tags --depth=1 origin "$base"
fi

if [ -n "$base" ] && git cat-file -e "$base:$guard" 2>/dev/null && git cat-file -e "$base:$hashes" 2>/dev/null; then
  git show "$base:$guard" > "$trusted/private-names-guard.py"
  git show "$base:$hashes" > "$trusted/private-names-hashes.txt"
  python3 -I "$trusted/private-names-guard.py"
fi

python3 -I "$guard"

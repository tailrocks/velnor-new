#!/bin/sh
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
#
# Lists the [[actions]] pins in .velnor/version-policy.toml whose eligible_on
# date has arrived: a newer release exists and has passed the 14-day window,
# so the pin should be bumped. Prints one line per due pin and exits 0.
#
# Usage: scripts/pins-eligible.sh [--today YYYY-MM-DD]
#        TODAY=YYYY-MM-DD scripts/pins-eligible.sh
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
policy="$root/.velnor/version-policy.toml"
today=${TODAY:-$(date -u +%F)}
if [ "${1:-}" = "--today" ]; then
  today=${2:?--today needs a YYYY-MM-DD date}
fi

exec python3 - "$policy" "$today" <<'PY'
import sys
import tomllib

path, today = sys.argv[1], sys.argv[2]
with open(path, "rb") as file:
    actions = tomllib.load(file).get("actions", [])
for action in actions:
    due = action.get("eligible_on")
    if due and due <= today:
        print(f"{action['name']} {action['version']} eligible_on {due}: "
              "a newer release is past the 14-day window; bump the pin")
PY

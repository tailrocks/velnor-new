#!/usr/bin/env bash
# Compare compiled-in pin constants against .velnor/freshness-inventory.json.
#
# Fail-closed: any missing file, unparseable constant, missing pin, or value
# mismatch exits non-zero. Live upstream rechecks (comparing the inventory
# against GitHub/crates.io latest releases) are OUT of scope for this script;
# it only proves the code constants equal the reviewed inventory.
#
# Usage: scripts/check-freshness.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INV="$ROOT/.velnor/freshness-inventory.json"

if [[ ! -f "$INV" ]]; then
  echo "check-freshness: missing inventory: $INV" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "check-freshness: python3 is required" >&2
  exit 1
fi

python3 - "$ROOT" "$INV" <<'EOF'
import json, re, sys

root, inv_path = sys.argv[1], sys.argv[2]
failures = []

def rust_const(path, name):
    """Extract `pub const NAME: &str = "value";` or record a failure."""
    try:
        text = open(f"{root}/{path}").read()
    except OSError as e:
        failures.append(f"{path}: unreadable ({e})")
        return None
    m = re.search(rf'pub const {name}:\s*&str\s*=\s*"([^"]*)";', text)
    if not m:
        failures.append(f"{path}: missing const {name}")
        return None
    return m.group(1)

def check(label, actual, expected):
    if actual is None:
        return  # already recorded
    if actual != expected:
        failures.append(f"{label}: code={actual!r} inventory={expected!r}")
    else:
        print(f"ok: {label} = {actual}")

try:
    inv = json.load(open(inv_path))
except (OSError, ValueError) as e:
    print(f"check-freshness: unreadable inventory ({e})", file=sys.stderr)
    sys.exit(1)

CATALOG = "crates/velnor-actions-mise/src/catalog.rs"
ACTIONS = "crates/velnor-actions-actionlint/src/actions.rs"
TOOLS = "crates/velnor-actions-actionlint/src/tools.rs"

# Tool pins live in the mise catalog.
tool_consts = {
    "mise": "MISE_VERSION",
    "rust": "RUST_VERSION",
    "mr-boxington": "MR_BOXINGTON_VERSION",
    "gh": "GH_VERSION",
    "actionlint": "ACTIONLINT_VERSION",
    "shellcheck": "SHELLCHECK_VERSION",
    "zizmor": "ZIZMOR_VERSION",
}
seen_tools = set()
for tool in inv.get("tools", []):
    name = tool.get("name")
    seen_tools.add(name)
    const = tool_consts.get(name)
    if const is None:
        failures.append(f"tools: inventory entry {name!r} has no mapped const")
        continue
    check(f"tool {name} ({CATALOG}::{const})",
          rust_const(CATALOG, const), tool.get("pinned"))
for name in sorted(set(tool_consts) - seen_tools):
    failures.append(f"tools: inventory missing pin for {name!r}")

# Mirror constants elsewhere must agree with the same inventory pin.
check("tool actionlint mirror (capabilities.rs)",
      rust_const("crates/velnor-actions-actionlint/src/capabilities.rs",
                 "ACTIONLINT_VERSION"),
      next(t["pinned"] for t in inv["tools"] if t["name"] == "actionlint"))
check("tool shellcheck mirror (tools.rs)",
      rust_const(TOOLS, "SHELLCHECK_VERSION"),
      next(t["pinned"] for t in inv["tools"] if t["name"] == "shellcheck"))

# Action pins live in the actionlint actions module.
action_consts = {
    "jdx/mise-action": "MISE_ACTION",
    "actions/checkout": "CHECKOUT_ACTION",
    "actions/download-artifact": "DOWNLOAD_ARTIFACT_ACTION",
    "actions/upload-artifact": "UPLOAD_ARTIFACT_ACTION",
    "actions/cache/restore": "CACHE_ACTION",
    "actions/cache/save": "CACHE_ACTION",
    "jdx/mr-boxington-action": "MR_BOXINGTON_ACTION",
}
for action in inv.get("actions", []):
    key = action.get("key")
    if key == "asamarts/alint":
        check(f"action {key} reviewed tag ({ACTIONS}::ALINT_REVIEWED_TAG)",
              rust_const(ACTIONS, "ALINT_REVIEWED_TAG"),
              action.get("pinned_tag"))
        continue
    prefix = action_consts.get(key)
    if prefix is None:
        failures.append(f"actions: inventory entry {key!r} has no mapped const")
        continue
    check(f"action {key} version ({ACTIONS}::{prefix}_VERSION)",
          rust_const(ACTIONS, f"{prefix}_VERSION"),
          action.get("pinned_version"))
    check(f"action {key} sha ({ACTIONS}::{prefix}_SHA)",
          rust_const(ACTIONS, f"{prefix}_SHA"),
          action.get("pinned_sha"))

# Runner default cross-check (bridge constant is owned elsewhere; read-only).
runner_default = (inv.get("runner") or {}).get("default")
check("runner default (config.rs::RUNNER_LABEL_BRIDGE)",
      rust_const("crates/velnor-actions-actionlint/src/config.rs",
                 "RUNNER_LABEL_BRIDGE"),
      runner_default)

if failures:
    print("check-freshness: FAIL", file=sys.stderr)
    for f in failures:
        print(f"  - {f}", file=sys.stderr)
    sys.exit(1)
print("check-freshness: PASS (code constants match inventory; "
      "live upstream recheck out of scope)")
EOF

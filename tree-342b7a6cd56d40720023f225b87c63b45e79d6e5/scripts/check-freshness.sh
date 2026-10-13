#!/usr/bin/env bash
# Compare compiled-in pin constants against .velnor/freshness-inventory.json,
# enforce temporary-hold expiry (<= max_exception_days), and probe Cargo.lock
# staleness (declared `=x.y.z` vs locked).
#
# Fail-closed: any missing file, unparseable constant, missing pin, value
# mismatch, expired/overlong hold, or declared-vs-locked skew exits non-zero.
# Live upstream rechecks (comparing the inventory against GitHub/crates.io
# latest releases) are OUT of scope for this script; it only proves the code
# constants equal the reviewed inventory. This script + the documented
# procedure (docs/implemented/update-procedure.md) is the mechanical maximum:
# no updater binary exists, and none is planned for V1.
#
# Machine-readable output: every `row: {...}` line on stdout is one compact
# JSON object with keys check/subject/status/detail. Human `ok:` lines and
# the final PASS/FAIL summary are for logs only.
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
import datetime, json, os, re, sys
import tomllib

root, inv_path = sys.argv[1], sys.argv[2]
failures = []

def row(check, subject, status, detail=""):
    """Machine-readable inventory row: `row: {compact JSON}` on stdout."""
    payload = json.dumps({"check": check, "subject": subject,
                          "status": status, "detail": detail},
                         separators=(",", ":"), sort_keys=True)
    print(f"row: {payload}")

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
    "nextest": "NEXTEST_VERSION",
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

# Policy-header consistency: .velnor/version-policy.toml MUST carry the §0
# header, and its cadence fields MUST equal the inventory's (VER-0.1).
try:
    with open(f"{root}/.velnor/version-policy.toml", "rb") as fh:
        policy = tomllib.load(fh)
except (OSError, tomllib.TOMLDecodeError) as e:
    failures.append(f"version-policy.toml: unreadable ({e})")
    policy = None
if policy is not None:
    for key, want in (("schema", 1), ("channel", "stable"),
                      ("check_interval_hours", inv.get("check_interval_hours")),
                      ("max_exception_days", inv.get("max_exception_days"))):
        got = policy.get(key)
        if got != want:
            failures.append(f"version-policy header: {key}={got!r} want {want!r}")
            row("policy-header", key, "fail", f"got={got!r} want={want!r}")
        else:
            print(f"ok: version-policy header {key} = {got!r}")
            row("policy-header", key, "pass", f"{got!r}")

# Exception-expiry enforcement (version-policy §1/§3): every temporary hold
# MUST name granted/expires/owner/reason, last no more than
# max_exception_days, and fail the run once expired. Standing exceptions are
# allowed only when explicitly spec-blessed with no expiry.
max_days = inv.get("max_exception_days")
today = datetime.date.today()
for hold in inv.get("temporary_holds", []):
    subject = hold.get("key", "<unnamed hold>")
    missing = [k for k in ("held_version", "owner", "reason", "granted",
                           "expires") if not hold.get(k)]
    if missing:
        failures.append(f"hold {subject}: missing {','.join(missing)}")
        row("exception-expiry", subject, "fail",
            f"missing {','.join(missing)}")
        continue
    try:
        granted = datetime.date.fromisoformat(hold["granted"])
        expires = datetime.date.fromisoformat(hold["expires"])
    except ValueError:
        failures.append(f"hold {subject}: granted/expires must be YYYY-MM-DD")
        row("exception-expiry", subject, "fail", "bad date format")
        continue
    span = (expires - granted).days
    if not isinstance(max_days, int) or span > max_days:
        failures.append(f"hold {subject}: span {span}d exceeds max {max_days}d")
        row("exception-expiry", subject, "fail", f"span {span}d > {max_days}d")
    elif expires < today:
        failures.append(f"hold {subject}: expired {expires} (renewal needs "
                        f"new review + evidence)")
        row("exception-expiry", subject, "fail", f"expired {expires}")
    else:
        print(f"ok: hold {subject} expires {expires} ({span}d <= {max_days}d)")
        row("exception-expiry", subject, "pass", f"expires {expires}")
if not inv.get("temporary_holds"):
    print("ok: no temporary holds")
    row("exception-expiry", "(none)", "pass", "no temporary holds")
for exc in inv.get("exceptions", []):
    subject = exc.get("key", "<unnamed exception>")
    if exc.get("expires") is None and exc.get("kind") and \
            exc.get("expiry_policy"):
        print(f"ok: standing exception {subject} ({exc.get('kind')})")
        row("standing-exception", subject, "pass", str(exc.get("kind")))
    elif exc.get("expires") is None:
        failures.append(f"exception {subject}: standing exception without "
                        f"kind+expiry_policy")
        row("standing-exception", subject, "fail", "unblessed standing hold")
    else:
        try:
            expires = datetime.date.fromisoformat(exc["expires"])
        except ValueError:
            failures.append(f"exception {subject}: expires must be YYYY-MM-DD")
            row("standing-exception", subject, "fail", "bad date format")
            continue
        if expires < today:
            failures.append(f"exception {subject}: expired {expires}")
            row("standing-exception", subject, "fail", f"expired {expires}")
        else:
            row("standing-exception", subject, "pass", f"expires {expires}")

# Lock-staleness probe (RQ-2.11, VER-2.26): every direct external dependency
# MUST declare an exact `=x.y.z` requirement, and each MUST equal the
# version pinned in Cargo.lock. Content equality is the gate; mtimes are
# informational only (spec-only manifest edits legitimately postdate the
# lock without changing the resolution).
try:
    with open(f"{root}/Cargo.lock", "rb") as fh:
        locked = {p["name"]: p["version"]
                  for p in tomllib.load(fh).get("package", [])}
except (OSError, tomllib.TOMLDecodeError, KeyError) as e:
    failures.append(f"Cargo.lock: unreadable ({e})")
    locked = None
if locked is not None:
    import glob
    manifests = sorted(glob.glob(f"{root}/crates/*/Cargo.toml"))
    checked = 0
    for manifest in manifests:
        try:
            with open(manifest, "rb") as fh:
                doc = tomllib.load(fh)
        except (OSError, tomllib.TOMLDecodeError) as e:
            failures.append(f"{manifest}: unreadable ({e})")
            continue
        crate = doc.get("package", {}).get("name", manifest)
        for section in ("dependencies", "dev-dependencies"):
            deps = doc.get(section, {})
            for dep, spec in sorted(deps.items()):
                if not isinstance(spec, dict) or "version" not in spec:
                    continue  # path/workspace dep: no registry version
                want = spec["version"]
                subject = f"{crate}:{dep}"
                if not want.startswith("="):
                    failures.append(f"{subject}: requirement {want!r} is not "
                                    f"exact `=x.y.z` (VER-2.26)")
                    row("lock-staleness", subject, "fail",
                        f"inexact {want!r}")
                    continue
                # Cargo ignores build metadata (+...) when matching.
                want_base = want[1:].split("+", 1)[0]
                got = (locked.get(dep) or "").split("+", 1)[0]
                checked += 1
                if got != want_base:
                    failures.append(f"{subject}: declared {want!r} != "
                                    f"locked {locked.get(dep)!r}")
                    row("lock-staleness", subject, "fail",
                        f"declared {want!r} locked {locked.get(dep)!r}")
                else:
                    print(f"ok: {subject} declared {want!r} == locked")
                    row("lock-staleness", subject, "pass", want)
    row("lock-staleness", "(summary)", "pass" if checked else "fail",
        f"{checked} exact direct deps match lock")
    try:
        lock_mtime = os.path.getmtime(f"{root}/Cargo.lock")
        newest_manifest = max(os.path.getmtime(m) for m in manifests)
        stale = lock_mtime < newest_manifest
        row("lock-mtime", "Cargo.lock", "info",
            f"lock_is_newest={str(not stale).lower()}")
    except OSError as e:
        row("lock-mtime", "Cargo.lock", "info", f"mtime unreadable ({e})")

if failures:
    print("check-freshness: FAIL", file=sys.stderr)
    for f in failures:
        print(f"  - {f}", file=sys.stderr)
    sys.exit(1)
print("check-freshness: PASS (code constants match inventory; "
      "live upstream recheck out of scope)")
EOF

#!/usr/bin/env bash
# Fail-closed freshness gate over the Velnor-owned version inventories.
#
# Validates four separated concerns (never conflated):
#   local-pin          compiled-in constants == reviewed inventory pins,
#                      including the version-policy mirror and the
#                      cargo-mutants activation pin.
#   effective-identity declared manifest requirements == each Cargo.lock
#                      resolution, keyed by name+version+source with every
#                      discovered workspace, dependency form, and scope.
#   upstream-freshness reviewed pins are backed by fresh upstream evidence
#                      (source URL + check timestamp); stale evidence, stale
#                      pins, and lookup failures fail, never report current.
#   advisories         deny policy forbids ignored advisories; the live
#                      `cargo deny` scan runs in CI (or `--with-advisories`).
#
# Machine-readable output: every `row: {...}` line on stdout is one compact
# JSON object with keys check/subject/status/detail. `status` is one of
# pass/fail/info. Every fail row contributes to a nonzero exit; human `ok:`
# lines and the final PASS/FAIL summary are for logs only.
#
# Usage: scripts/check-freshness.sh [--root DIR] [--check-upstream]
#                                   [--with-advisories]
#   --root DIR         validate a fixture tree instead of this repository.
#   --check-upstream   bounded read-only upstream probe: refetch each row's
#                      latest stable release (10 s timeout and 512 KiB cap
#                      per request) and fail stale pins and lookup failures.
#                      Writes nothing; run by the generated weekly
#                      `.github/workflows/freshness.yml`, never gating builds.
#   --with-advisories  run the live `cargo deny check advisories` scan for
#                      each workspace (180 s each) plus the policy checks.
set -euo pipefail

ROOT=""
CHECK_UPSTREAM=0
WITH_ADVISORIES=0

usage() {
  echo "usage: scripts/check-freshness.sh [--root DIR] [--check-upstream] [--with-advisories]"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)
      if [[ $# -lt 2 ]]; then
        echo "check-freshness: --root needs a directory" >&2
        exit 2
      fi
      ROOT="$2"
      shift 2
      ;;
    --check-upstream)
      CHECK_UPSTREAM=1
      shift
      ;;
    --with-advisories)
      WITH_ADVISORIES=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "check-freshness: unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$ROOT" ]]; then
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fi
INV="$ROOT/.velnor/freshness-inventory.json"

if [[ ! -f "$INV" ]]; then
  echo "check-freshness: missing inventory: $INV" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "check-freshness: python3 is required" >&2
  exit 1
fi

python3 - "$ROOT" "$INV" "$CHECK_UPSTREAM" "$WITH_ADVISORIES" <<'EOF'
import datetime
import glob as globmod
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
import urllib.request

root, inv_path = sys.argv[1], sys.argv[2]
check_upstream = sys.argv[3] == "1"
with_advisories = sys.argv[4] == "1"
failures = []

NOW = datetime.datetime.now(datetime.timezone.utc)


def row(check, subject, status, detail=""):
    """Machine-readable inventory row: `row: {compact JSON}` on stdout."""
    payload = json.dumps({"check": check, "subject": subject,
                          "status": status, "detail": detail},
                         separators=(",", ":"), sort_keys=True)
    print(f"row: {payload}")


def fail_row(check, subject, detail):
    """A failed inventory row; every one of these exits the run nonzero."""
    row(check, subject, "fail", detail)
    failures.append(f"{check} {subject}: {detail}")


def pass_row(check, subject, detail=""):
    row(check, subject, "pass", detail)
    print(f"ok: {check} {subject} {detail}".rstrip())


def info_row(check, subject, detail=""):
    row(check, subject, "info", detail)


def rust_const(path, name):
    """Extract `pub const NAME: &str = "value";` or emit a fail row."""
    try:
        with open(f"{root}/{path}", encoding="utf-8") as handle:
            text = handle.read()
    except OSError as err:
        fail_row("local-pin", f"{path}::{name}", f"unreadable ({err})")
        return None
    match = re.search(rf'pub const {name}:\s*&str\s*=\s*"([^"]*)";', text)
    if not match:
        fail_row("local-pin", f"{path}::{name}", f"missing const {name}")
        return None
    return match.group(1)


def parse_iso_date(text):
    """Strict `YYYY-MM-DD` date or None."""
    if not isinstance(text, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", text):
        return None
    try:
        return datetime.date.fromisoformat(text)
    except ValueError:
        return None


def norm_version(text):
    """Compare versions ignoring a leading `v` and build metadata."""
    return (text or "").strip().removeprefix("v").split("+", 1)[0]


def parse_timestamp(text):
    """Evidence timestamp (date or datetime) as aware UTC datetime or None."""
    if not isinstance(text, str) or not text.strip():
        return None
    candidate = text.strip().replace("Z", "+00:00")
    try:
        parsed = datetime.datetime.fromisoformat(candidate)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=datetime.timezone.utc)
    return parsed.astimezone(datetime.timezone.utc)


try:
    with open(inv_path, encoding="utf-8") as handle:
        inv = json.load(handle)
except (OSError, ValueError) as err:
    print(f"check-freshness: unreadable inventory ({err})", file=sys.stderr)
    sys.exit(1)

if not isinstance(inv, dict):
    print("check-freshness: inventory root must be an object", file=sys.stderr)
    sys.exit(1)

# The sibling-owned inventory may grow new keys; specified fields below are
# validated strictly and anything unrecognized is reported, not rejected.
for key in sorted(inv):
    if key not in ("schema", "check_interval_hours", "max_exception_days",
                   "checked_at", "tools", "actions", "runner", "exceptions",
                   "temporary_holds"):
        info_row("inventory-shape", key, "unrecognized top-level key")

if inv.get("schema") != 1:
    fail_row("inventory-shape", "schema", f"must be 1, got {inv.get('schema')!r}")
interval = inv.get("check_interval_hours")
max_days = inv.get("max_exception_days")
if not isinstance(interval, int) or interval <= 0:
    fail_row("inventory-shape", "check_interval_hours",
             f"must be a positive int, got {interval!r}")
    interval = 24
if not isinstance(max_days, int) or max_days <= 0:
    fail_row("inventory-shape", "max_exception_days",
             f"must be a positive int, got {max_days!r}")
    max_days = 14
top_checked = inv.get("checked_at")
top_evidence = parse_timestamp(top_checked) if top_checked is not None else None
if top_checked is not None and top_evidence is None:
    fail_row("inventory-shape", "checked_at",
             f"malformed timestamp {top_checked!r}")

# --- Version-policy header (§0): strict shape, no weakening, no unknowns.
try:
    with open(f"{root}/.velnor/version-policy.toml", "rb") as handle:
        policy = tomllib.load(handle)
except (OSError, tomllib.TOMLDecodeError) as err:
    fail_row("policy-header", "version-policy.toml", f"unreadable ({err})")
    policy = None
if policy is not None:
    for key in sorted(policy):
        if key in ("tools", "github_runner_images", "actions",
                   "validation-tools"):
            continue
        if key not in ("schema", "channel", "registry",
                       "check_interval_hours", "max_exception_days"):
            fail_row("policy-header", key, "unknown key rejected")
    header = (("schema", 1), ("channel", "stable"),
              ("registry", policy.get("registry")),
              ("check_interval_hours", interval),
              ("max_exception_days", max_days))
    for key, want in header:
        got = policy.get(key)
        if key == "registry":
            if not isinstance(got, str) or not got.startswith("https://") \
                    or any(char.isspace() for char in got):
                fail_row("policy-header", key,
                         f"must be an https URL, got {got!r}")
            else:
                pass_row("policy-header", key, got)
            continue
        if got != want:
            fail_row("policy-header", key, f"got={got!r} want={want!r}")
        else:
            pass_row("policy-header", key, f"{got!r}")
    if isinstance(policy.get("check_interval_hours"), int) \
            and policy["check_interval_hours"] > 24:
        fail_row("policy-header", "check_interval_hours",
                 "weakens policy: must be <= 24")
    if isinstance(policy.get("max_exception_days"), int) \
            and policy["max_exception_days"] > 14:
        fail_row("policy-header", "max_exception_days",
                 "weakens policy: must be <= 14")

# --- Local pins: code constants == reviewed inventory pins (VER-0.1).
CATALOG = "crates/adapters/velnor-actions-mise/src/catalog.rs"
ACTIONS = "crates/adapters/velnor-actions-actionlint/src/actions.rs"
TOOLS = "crates/adapters/velnor-actions-actionlint/src/tools.rs"
CAPABILITIES = "crates/adapters/velnor-actions-actionlint/src/capabilities.rs"
CONFIG = "crates/adapters/velnor-actions-actionlint/src/config.rs"
RENDERER = "crates/services/velnor-actions-workflow-renderer/src/render.rs"

EXPECTED_TOOLS = {
    "mise": "MISE_VERSION",
    "rust": "RUST_VERSION",
    "mr-boxington": "MR_BOXINGTON_VERSION",
    "gh": "GH_VERSION",
    "actionlint": "ACTIONLINT_VERSION",
    "shellcheck": "SHELLCHECK_VERSION",
    "zizmor": "ZIZMOR_VERSION",
    "nextest": "NEXTEST_VERSION",
    "opentofu": "OPENTOFU_VERSION",
    "release-plz": "RELEASE_PLZ_VERSION",
}
EXPECTED_ACTIONS = {
    "jdx/mise-action": "MISE_ACTION",
    "actions/checkout": "CHECKOUT_ACTION",
    "actions/download-artifact": "DOWNLOAD_ARTIFACT_ACTION",
    "actions/upload-artifact": "UPLOAD_ARTIFACT_ACTION",
    "actions/cache/restore": "CACHE_ACTION",
    "actions/cache/save": "CACHE_ACTION",
    "jdx/mr-boxington-action": "MR_BOXINGTON_ACTION",
    "asamarts/alint": "ALINT_ACTION",
    "Swatinem/rust-cache": "RUST_CACHE_ACTION",
}


def pin_row(subject, actual, expected):
    if actual is None:
        return  # rust_const already emitted the fail row
    if actual != expected:
        fail_row("local-pin", subject,
                 f"code={actual!r} inventory={expected!r}")
    else:
        pass_row("local-pin", subject, str(actual))


tools = inv.get("tools", [])
seen_tools = set()
for tool in tools:
    if not isinstance(tool, dict):
        fail_row("inventory-shape", "(inventory tools)",
                 f"entry must be an object, got {tool!r}")
        continue
    name = tool.get("name")
    seen_tools.add(name)
    const = EXPECTED_TOOLS.get(name)
    if const is None:
        fail_row("local-pin", f"tool {name}",
                 "inventory entry outside the expected tool set")
        continue
    pin_row(f"tool {name} ({CATALOG}::{const})",
            rust_const(CATALOG, const), tool.get("pinned"))
for name in sorted(set(EXPECTED_TOOLS) - seen_tools):
    fail_row("local-pin", f"tool {name}", "inventory row missing")

action_pinned = {}
for action in inv.get("actions", []):
    if not isinstance(action, dict):
        fail_row("inventory-shape", "(inventory actions)",
                 f"entry must be an object, got {action!r}")
        continue
    key = action.get("key")
    action_pinned[key] = action
    prefix = EXPECTED_ACTIONS.get(key)
    if prefix is None:
        fail_row("local-pin", f"action {key}",
                 "inventory entry outside the expected action set")
        continue
    pin_row(f"action {key} version ({ACTIONS}::{prefix}_VERSION)",
            rust_const(ACTIONS, f"{prefix}_VERSION"),
            action.get("pinned_version"))
    pin_row(f"action {key} sha ({ACTIONS}::{prefix}_SHA)",
            rust_const(ACTIONS, f"{prefix}_SHA"),
            action.get("pinned_sha"))
for key in sorted(set(EXPECTED_ACTIONS) - set(action_pinned)):
    fail_row("local-pin", f"action {key}", "inventory row missing")

tool_pinned = {tool.get("name"): tool.get("pinned") for tool in tools}
pin_row("tool actionlint mirror (capabilities.rs)",
        rust_const(CAPABILITIES, "ACTIONLINT_VERSION"),
        tool_pinned.get("actionlint"))
pin_row("tool shellcheck mirror (tools.rs)",
        rust_const(TOOLS, "SHELLCHECK_VERSION"),
        tool_pinned.get("shellcheck"))
pin_row("action asamarts/alint binary mirror (render.rs)",
        rust_const(RENDERER, "ALINT_BINARY_VERSION"),
        action_pinned.get("asamarts/alint", {}).get("pinned_version"))

runner = inv.get("runner") or {}
pin_row("runner default (config.rs::RUNNER_LABEL_BRIDGE)",
        rust_const(CONFIG, "RUNNER_LABEL_BRIDGE"), runner.get("default"))
supported = runner.get("supported", [])
if not isinstance(supported, list) or not supported:
    fail_row("local-pin", "runner supported", "must be a non-empty label list")
else:
    for label in supported:
        if not isinstance(label, str) or "latest" in label:
            fail_row("local-pin", "runner supported",
                     f"unversioned label rejected: {label!r}")
    if runner.get("default") not in supported:
        fail_row("local-pin", "runner supported",
                 f"default {runner.get('default')!r} not listed")
    else:
        pass_row("local-pin", "runner supported", ",".join(supported))

# --- Policy mirror: version-policy.toml == inventory (VER-2 mirror).
if policy is not None:
    policy_tools = policy.get("tools", {})
    for name in sorted(set(EXPECTED_TOOLS) | set(policy_tools)):
        want = tool_pinned.get(name)
        got = policy_tools.get(name)
        if name not in EXPECTED_TOOLS:
            fail_row("policy-mirror", f"tool {name}",
                     "policy entry outside the expected tool set")
        elif got != want:
            fail_row("policy-mirror", f"tool {name}",
                     f"policy={got!r} inventory={want!r}")
        else:
            pass_row("policy-mirror", f"tool {name}", str(got))
    images = (policy.get("github_runner_images") or {}).get("linux_x64") or {}
    if images.get("default") != runner.get("default"):
        fail_row("policy-mirror", "runner default",
                 f"policy={images.get('default')!r} "
                 f"inventory={runner.get('default')!r}")
    else:
        pass_row("policy-mirror", "runner default",
                 str(images.get("default")))
    if sorted(images.get("supported", [])) != sorted(supported):
        fail_row("policy-mirror", "runner supported",
                 f"policy={images.get('supported')!r} "
                 f"inventory={supported!r}")
    else:
        pass_row("policy-mirror", "runner supported",
                 ",".join(images.get("supported", [])))
    policy_actions = {}
    for entry in policy.get("actions", []):
        if not isinstance(entry, dict):
            fail_row("policy-mirror", "actions",
                     f"entry must be a table, got {entry!r}")
            continue
        for key in sorted(entry):
            if key not in ("name", "version", "sha", "reviewed"):
                fail_row("policy-mirror", f"action {entry.get('name')}",
                         f"unknown key rejected: {key}")
        name = entry.get("name")
        if name in policy_actions:
            fail_row("policy-mirror", f"action {name}", "duplicate entry")
        policy_actions[name] = entry
        stamp = parse_iso_date(entry.get("reviewed", ""))
        if stamp is None:
            fail_row("policy-mirror", f"action {name}",
                     f"reviewed must be YYYY-MM-DD, "
                     f"got {entry.get('reviewed')!r}")
    for key in sorted(set(action_pinned) | set(policy_actions)):
        if key not in action_pinned:
            fail_row("policy-mirror", f"action {key}",
                     "policy entry without an inventory row")
            continue
        if key not in policy_actions:
            fail_row("policy-mirror", f"action {key}",
                     "inventory row without a policy entry")
            continue
        want = action_pinned[key]
        got = policy_actions[key]
        if got.get("version") != want.get("pinned_version") or \
                got.get("sha") != want.get("pinned_sha"):
            fail_row("policy-mirror", f"action {key}",
                     f"policy={got.get('version')!r}@{got.get('sha')!r} "
                     f"inventory={want.get('pinned_version')!r}@"
                     f"{want.get('pinned_sha')!r}")
        elif not re.fullmatch(r"[0-9a-f]{40}", got.get("sha") or ""):
            fail_row("policy-mirror", f"action {key}",
                     f"sha must be 40 hex, got {got.get('sha')!r}")
        else:
            pass_row("policy-mirror", f"action {key}",
                     f"{got.get('version')}@{got.get('sha')}")

# --- Validation-tool pins: activated manual tools are pinned first (§2).
MUTANTS = ".cargo/mutants.toml"
try:
    with open(f"{root}/{MUTANTS}", encoding="utf-8") as handle:
        mutants_text = handle.read()
except OSError as err:
    fail_row("local-pin", MUTANTS, f"unreadable ({err})")
    mutants_text = None
if mutants_text is not None and policy is not None:
    pinned_tools = policy.get("validation-tools", {})
    if "cargo-mutants" not in pinned_tools:
        fail_row("local-pin", "validation-tools/cargo-mutants",
                 "policy pin missing")
    else:
        want = pinned_tools["cargo-mutants"]
        match = re.search(r'^# pinned: cargo-mutants = "([^"]+)"',
                          mutants_text, re.MULTILINE)
        if not match:
            fail_row("local-pin", "validation-tools/cargo-mutants",
                     f"{MUTANTS} lacks a `# pinned: cargo-mutants = \"x\"` line")
        elif match.group(1) != want:
            fail_row("local-pin", "validation-tools/cargo-mutants",
                     f"mutants pin={match.group(1)!r} policy={want!r}")
        else:
            pass_row("local-pin", "validation-tools/cargo-mutants", want)
    in_scope, globs = False, []
    for line in mutants_text.splitlines():
        stripped = line.strip()
        if stripped.startswith("examine_globs"):
            in_scope = True
            continue
        if in_scope:
            if stripped.startswith("]"):
                break
            quoted = re.findall(r'"([^"]+)"', stripped.split("#")[0])
            globs.extend(quoted)
    if not globs:
        fail_row("local-pin", f"{MUTANTS} examine_globs", "scope is empty")
    for pattern in sorted(globs):
        hits = globmod.glob(f"{root}/{pattern}", recursive=True)
        if not [hit for hit in hits if os.path.isfile(hit)]:
            fail_row("local-pin", f"{MUTANTS} scope {pattern}",
                     "glob matches no production file")
        else:
            pass_row("local-pin", f"{MUTANTS} scope {pattern}",
                     f"{len(hits)} match(es)")

# --- Effective identity: declared == locked by name+version+source.
# Every workspace lock is evaluated independently. Nested virtual workspaces
# are discovered from the parent's explicit Cargo workspace.exclude list;
# each workspace's member declarations then select the manifests under its
# own dependency-inheritance and lockfile scope.
DEP_SECTIONS = ("dependencies", "dev-dependencies", "build-dependencies")
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"


def walk_dep_tables(node, prefix):
    """Yield (scope, key, spec) for every dependency table under node."""
    found = []
    if not isinstance(node, dict):
        return found
    for section in DEP_SECTIONS:
        table = node.get(section)
        if isinstance(table, dict):
            scope = f"{prefix}{section}" if prefix else section
            for key in sorted(table):
                found.append((scope, key, table[key]))
    target = node.get("target")
    if isinstance(target, dict):
        for name in sorted(target):
            found.extend(walk_dep_tables(target[name], f"target.{name}."))
    return found


def lock_identity(entry):
    name = entry.get("name", "")
    version = (entry.get("version") or "").split("+", 1)[0]
    source = entry.get("source") or "local"
    return (name, version, source)


def discover_workspaces():
    """Read the root workspace and nested workspaces it explicitly excludes."""
    roots = []
    pending = [""]
    seen = set()
    while pending:
        relative = pending.pop(0)
        if relative in seen:
            continue
        seen.add(relative)
        manifest = os.path.join(root, relative, "Cargo.toml")
        try:
            with open(manifest, "rb") as handle:
                doc = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as err:
            fail_row("lock-staleness", manifest, f"unreadable ({err})")
            continue
        workspace = doc.get("workspace")
        if not isinstance(workspace, dict):
            fail_row("lock-staleness", manifest, "no [workspace] table")
            continue
        roots.append((relative, doc))
        excludes = workspace.get("exclude", [])
        if not isinstance(excludes, list):
            fail_row("lock-staleness", manifest,
                     "workspace.exclude must be an array")
            continue
        for excluded in excludes:
            if not isinstance(excluded, str):
                fail_row("lock-staleness", manifest,
                         f"workspace.exclude entry is not a string: {excluded!r}")
                continue
            pattern = os.path.join(root, relative, excluded)
            for hit in globmod.glob(pattern, recursive=True):
                nested_manifest = (
                    hit if os.path.isfile(hit) and
                    os.path.basename(hit) == "Cargo.toml"
                    else os.path.join(hit, "Cargo.toml")
                )
                if not os.path.isfile(nested_manifest):
                    continue
                nested_relative = os.path.relpath(
                    os.path.dirname(nested_manifest), root
                )
                if nested_relative == "." or nested_relative in seen:
                    continue
                try:
                    with open(nested_manifest, "rb") as handle:
                        nested_doc = tomllib.load(handle)
                except (OSError, tomllib.TOMLDecodeError) as err:
                    fail_row("lock-staleness", nested_manifest,
                             f"unreadable ({err})")
                    continue
                if isinstance(nested_doc.get("workspace"), dict):
                    pending.append(nested_relative)
    return roots


def check_workspace_identity(relative, workspace_doc):
    """Check one Cargo workspace against only its own lockfile."""
    label = relative or "."
    workspace_dir = os.path.join(root, relative)
    manifest = os.path.join(workspace_dir, "Cargo.toml")
    lock_path = os.path.join(workspace_dir, "Cargo.lock")
    workspace = workspace_doc.get("workspace") or {}
    inherited = workspace.get("dependencies", {})
    if not isinstance(inherited, dict):
        fail_row("lock-staleness", f"{label}/Cargo.toml",
                 "workspace.dependencies must be a table")
        inherited = None
    try:
        with open(lock_path, "rb") as handle:
            locked = tomllib.load(handle).get("package", [])
    except (OSError, tomllib.TOMLDecodeError, AttributeError) as err:
        fail_row("lock-staleness", os.path.relpath(lock_path, root),
                 f"unreadable ({err})")
        return None
    if inherited is None:
        return None

    manifests = []
    if isinstance(workspace_doc.get("package"), dict):
        manifests.append(manifest)
    members = workspace.get("members", [])
    if not isinstance(members, list):
        fail_row("lock-staleness", f"{label}/Cargo.toml",
                 "workspace.members must be an array")
        members = []
    for member in members:
        if not isinstance(member, str):
            fail_row("lock-staleness", f"{label}/Cargo.toml",
                     f"workspace member is not a string: {member!r}")
            continue
        pattern = os.path.join(workspace_dir, member, "Cargo.toml")
        matches = globmod.glob(pattern, recursive=True)
        if not matches:
            fail_row("lock-staleness", f"{label}/{member}",
                     "workspace member manifest not found")
        manifests.extend(matches)
    manifests = sorted(set(manifests))
    if not manifests:
        fail_row("lock-staleness", f"{label}/Cargo.toml",
                 "no workspace member manifests found")

    member_names = set()
    declared = []  # (crate, scope, alias, real, req, detail)
    for member_manifest in manifests:
        try:
            with open(member_manifest, "rb") as handle:
                doc = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as err:
            fail_row("lock-staleness", member_manifest, f"unreadable ({err})")
            continue
        package = doc.get("package")
        if not isinstance(package, dict) or not package.get("name"):
            fail_row("lock-staleness", member_manifest,
                     "workspace member has no package name")
            continue
        crate = package["name"]
        member_names.add(crate)
        for scope, alias, spec in walk_dep_tables(doc, ""):
            subject = f"{crate}:{scope}:{alias}"
            if isinstance(spec, str):
                declared.append((subject, alias, spec, None))
            elif isinstance(spec, dict):
                if spec.get("git"):
                    fail_row("lock-staleness", subject,
                             f"git dependency forbidden ({spec.get('git')!r})")
                    continue
                real = spec.get("package", alias)
                if "version" in spec:
                    declared.append((subject, real, spec["version"], None))
                elif spec.get("workspace") is True:
                    base = inherited.get(real, inherited.get(alias))
                    if isinstance(base, dict) and \
                            isinstance(base.get("package"), str):
                        real = base["package"]
                    req = None
                    if isinstance(base, str):
                        req = base
                    elif isinstance(base, dict) and "version" in base:
                        req = base["version"]
                    if req is None:
                        fail_row("lock-staleness", subject,
                                 "workspace inheritance unresolvable: "
                                 f"[workspace.dependencies] lacks {real!r}")
                        continue
                    declared.append((subject, real, req, "workspace"))
                elif "path" in spec:
                    pass_row("lock-staleness", subject,
                             "path-only, no registry identity")
                else:
                    fail_row("lock-staleness", subject,
                             f"no version, workspace, or path in {spec!r}")
            else:
                fail_row("lock-staleness", subject,
                         f"malformed spec {spec!r}")

    by_name = {}
    for entry in locked:
        by_name.setdefault(entry.get("name"), []).append(entry)
    passed = failed = 0
    for subject, real, req, _ in declared:
        if not isinstance(req, str) or not req.startswith("="):
            fail_row("lock-staleness", subject,
                     f"requirement {req!r} is not exact `=x.y.z` (VER-2.26)")
            failed += 1
            continue
        want = req[1:].split("+", 1)[0]
        matches = [entry for entry in by_name.get(real, [])
                   if (entry.get("version") or "").split("+", 1)[0] == want]
        if not matches:
            have = sorted({(entry.get("version") or "")
                           for entry in by_name.get(real, [])})
            fail_row("lock-staleness", subject,
                     f"declared {req!r} has no locked identity "
                     f"(locked versions: {have})")
            failed += 1
            continue
        sources = {entry.get("source") or "local" for entry in matches}
        if len(sources) > 1:
            fail_row("lock-staleness", subject,
                     f"ambiguous identity: {real} {want} resolves from "
                     f"{sorted(sources)}")
            failed += 1
            continue
        source = next(iter(sources))
        if real in member_names:
            if source != "local":
                fail_row("lock-staleness", subject,
                         f"workspace member {real} locked from {source}")
                failed += 1
            else:
                pass_row("lock-staleness", subject, f"{req} @ workspace")
                passed += 1
        elif source != CRATES_IO:
            fail_row("lock-staleness", subject,
                     f"locked from non-registry source {source}")
            failed += 1
        else:
            pass_row("lock-staleness", subject, f"{req} @ registry")
            passed += 1

    info_row("lock-staleness", f"{label}/(declared-summary)",
             f"{passed} match, {failed} fail, "
             f"{len(by_name)} locked names retained")
    local_names = {entry.get("name") for entry in locked
                   if not entry.get("source")}
    if local_names != member_names:
        fail_row("lock-staleness", f"{label}/(lock-membership)",
                 f"local lock {sorted(local_names)} != "
                 f"members {sorted(member_names)}")
    else:
        pass_row("lock-staleness", f"{label}/(lock-membership)",
                 f"{len(member_names)} members")
    queue = [entry for name in sorted(local_names & member_names)
             for entry in by_name.get(name, [])]
    reachable = {lock_identity(entry) for entry in queue}
    while queue:
        entry = queue.pop()
        for edge in entry.get("dependencies", []) or []:
            parts = edge.split(" ")
            candidates = by_name.get(parts[0], [])
            if len(parts) > 1:
                candidates = [item for item in candidates
                              if (item.get("version") or "") == parts[1]]
            if len(parts) > 2:
                want_source = parts[2].strip("()")
                candidates = [item for item in candidates
                              if (item.get("source") or "") == want_source]
            if not candidates:
                fail_row("lock-staleness", f"{label}/(lock-graph)",
                         f"dangling edge {entry.get('name')} -> {edge}")
                continue
            for candidate in candidates:
                ident = lock_identity(candidate)
                if ident not in reachable:
                    reachable.add(ident)
                    queue.append(candidate)
    stranded = [entry for entry in locked
                if lock_identity(entry) not in reachable]
    if stranded:
        for entry in sorted(stranded, key=lambda item: item.get("name", "")):
            fail_row("lock-staleness", f"{label}/(lock-graph)",
                     f"unreachable locked package "
                     f"{entry.get('name')} {entry.get('version')}")
    else:
        pass_row("lock-staleness", f"{label}/(lock-graph)",
                 f"{len(locked)} locked packages reachable")
    try:
        lock_mtime = os.path.getmtime(lock_path)
        newest_manifest = max(os.path.getmtime(item) for item in manifests)
        stale = lock_mtime < newest_manifest
        info_row("lock-mtime", os.path.relpath(lock_path, root),
                 f"lock_is_newest={str(not stale).lower()}")
    except OSError as err:
        info_row("lock-mtime", os.path.relpath(lock_path, root),
                 f"mtime unreadable ({err})")
    return locked


workspace_roots = discover_workspaces()
locked = []
for workspace_relative, workspace_doc in workspace_roots:
    workspace_locked = check_workspace_identity(workspace_relative, workspace_doc)
    if workspace_locked is not None:
        locked.extend(workspace_locked)
if not locked:
    info_row("lock-staleness", "(all workspaces)",
             "no lock packages loaded")
# --- Upstream freshness: pins need fresh evidence, never bare equality.
# status=current requires qualified==pinned AND a check timestamp within
# check_interval_hours. Anything else (stale evidence, stale pin, lookup
# failure) fails; a failure is never reported as current (§3.4).
holds = inv.get("temporary_holds", [])
hold_keys = {hold.get("key") for hold in holds if isinstance(hold, dict)}


def evidence_age_hours(entry):
    stamp = entry.get("checked_at", top_checked)
    moment = parse_timestamp(stamp)
    if moment is None:
        return (None, stamp)
    return ((NOW - moment).total_seconds() / 3600, stamp)


def future_evidence(stamp):
    """True when evidence is after now.

    A date-only stamp is future when its UTC day is after today. A stamp
    with a clock keeps a 0.1 hour skew grace.
    """
    text = stamp.strip() if isinstance(stamp, str) else ""
    moment = parse_timestamp(text)
    if moment is None:
        return False
    date_only = (
        len(text) == 10 and text[4:5] == "-" and text[7:8] == "-"
        and "T" not in text and ":" not in text
    )
    if date_only:
        return moment.date() > NOW.date()
    return (NOW - moment).total_seconds() / 3600 < -0.1


def freshness_row(subject, entry, pinned, qualified, latest=None,
                  pin_for_latest=None):
    source = entry.get("source", "")
    if not isinstance(source, str) or "://" not in source:
        fail_row("upstream-freshness", subject, "missing source URL")
        return
    if not source.startswith(("https://", "http://", "file://")):
        fail_row("upstream-freshness", subject,
                 f"unsupported source scheme: {source!r}")
        return
    age, stamp = evidence_age_hours(entry)
    if age is None:
        fail_row("upstream-freshness", subject,
                 f"missing or malformed check timestamp {stamp!r}")
        return
    if future_evidence(stamp):
        fail_row("upstream-freshness", subject,
                 f"check timestamp {stamp} is in the future")
        return
    status = entry.get("status")
    if status == "held":
        if subject not in hold_keys and entry.get("key", subject) not in hold_keys:
            fail_row("upstream-freshness", subject,
                     "status=held without a covering temporary hold")
        else:
            pass_row("upstream-freshness", subject, f"held, evidence {stamp}")
        return
    if status != "current":
        fail_row("upstream-freshness", subject,
                 f"status={status!r}: refresh required, never current "
                 f"(source {source}, checked {stamp})")
        return
    if age > interval:
        fail_row("upstream-freshness", subject,
                 f"stale evidence: checked {stamp} ({age:.1f}h ago, "
                 f"interval {interval}h)")
        return
    if qualified != pinned:
        fail_row("upstream-freshness", subject,
                 f"unqualified pin: pinned={pinned!r} qualified={qualified!r}")
        return
    if latest is not None and norm_version(latest) != \
            norm_version(pin_for_latest if pin_for_latest is not None
                         else (pinned if isinstance(pinned, str) else "")):
        fail_row("upstream-freshness", subject,
                 f"stale pin: pinned={pinned!r} latest={latest!r} "
                 f"(source {source}, checked {stamp})")
        return
    pass_row("upstream-freshness", subject,
             f"current, evidence {stamp}")


for tool in tools:
    name = tool.get("name")
    freshness_row(name, tool, tool.get("pinned"), tool.get("qualified"),
                  tool.get("latest"))
for key, action in sorted(action_pinned.items()):
    pinned = (action.get("pinned_version"), action.get("pinned_sha"))
    qualified = (action.get("qualified_version"),
                 action.get("qualified_sha"))
    freshness_row(key, action, pinned, qualified, action.get("latest"),
                  action.get("pinned_version"))
freshness_row("runner", runner, runner.get("default"), runner.get("default"))

# --- Exceptions: hard maxima, full attribution, strict chronology (§1).
# UTC date from the single NOW source: local midnight differs from UTC
# midnight, and expiry arithmetic must match UTC evidence timestamps.
today = NOW.date()
lock_names = {entry.get("name") for entry in (locked or [])}
known_subjects = set(EXPECTED_TOOLS) | set(EXPECTED_ACTIONS) | \
    lock_names | set(supported)
if runner.get("default"):
    known_subjects.add(runner.get("default"))
def check_dated(entry, check):
    """Full attribution + chronology gate for dated exceptions/holds."""
    if not isinstance(entry, dict):
        fail_row(check, "(inventory exceptions)",
                 f"entry must be an object, got {entry!r}")
        return
    subject = entry.get("key", "<unnamed hold>")
    missing = [key for key in ("held_version", "owner", "issue", "reason",
                               "granted", "expires") if not entry.get(key)]
    if missing:
        fail_row(check, subject, f"missing {','.join(missing)}")
        return
    granted = parse_iso_date(entry["granted"])
    expires = parse_iso_date(entry["expires"])
    if granted is None or expires is None:
        fail_row(check, subject, "granted/expires must be YYYY-MM-DD")
        return
    if granted > today:
        fail_row(check, subject, f"granted {granted} is in the future")
    elif expires <= granted:
        fail_row(check, subject,
                 f"inverted window: expires {expires} <= granted {granted}")
    elif (expires - granted).days > max_days:
        fail_row(check, subject,
                 f"span {(expires - granted).days}d exceeds max {max_days}d")
    elif expires < today:
        fail_row(check, subject,
                 f"expired {expires} (renewal needs new review + evidence)")
    elif subject not in known_subjects:
        fail_row(check, subject,
                 "hold subject matches no inventoried tool, action, "
                 "runner label, or locked package")
    else:
        pass_row(check, subject, f"expires {expires}")


for hold in holds:
    check_dated(hold, "exception-expiry")
if not holds:
    pass_row("exception-expiry", "(none)", "no temporary holds")

# The one permitted standing-record key: `asamarts/alint` only. No standing
# record is present (inventory `exceptions: []`; the renderer pins Alint by
# full SHA like every other action); this gate constrains any future record.
# Its `tag` must equal the reviewed `pinned_version` of the inventory's
# alint action row, so a pin move without a re-blessing fails. Dated
# `exceptions` entries carry the same full attribution as holds but never
# cover `status: held` rows.
BLESSED_STANDING = "asamarts/alint"
reviewed_alint = (action_pinned.get(BLESSED_STANDING) or {}).get(
    "pinned_version")
for exc in inv.get("exceptions", []):
    if not isinstance(exc, dict):
        fail_row("standing-exception", "(inventory exceptions)",
                 f"entry must be an object, got {exc!r}")
        continue
    subject = exc.get("key", "<unnamed hold>")
    if exc.get("expires") is None:
        if subject != BLESSED_STANDING:
            fail_row("standing-exception", subject,
                     "standing hold without a spec blessing "
                     "(only asamarts/alint is blessed)")
        else:
            missing = [key for key in ("kind", "expiry_policy",
                                       "blessed_by", "tag")
                       if not exc.get(key)]
            if missing:
                fail_row("standing-exception", subject,
                         f"blessed standing exception lacks "
                         f"{','.join(missing)}")
            elif exc.get("tag") != reviewed_alint:
                fail_row("standing-exception", subject,
                         f"blessed tag {exc.get('tag')!r} != reviewed pin "
                         f"{reviewed_alint!r}: re-bless on pin moves")
            else:
                pass_row("standing-exception", subject,
                         f"blessed mutable tag {exc.get('tag')}")
    else:
        check_dated(exc, "standing-exception")

# --- Advisories: each workspace's deny policy plus the optional live scans.
for workspace_relative, _ in workspace_roots:
    workspace_dir = os.path.join(root, workspace_relative)
    manifest_path = os.path.join(workspace_dir, "Cargo.toml")
    deny_path = os.path.join(workspace_dir, "deny.toml")
    deny_subject = os.path.relpath(deny_path, root)
    try:
        with open(deny_path, "rb") as handle:
            deny = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as err:
        fail_row("advisories", deny_subject, f"unreadable ({err})")
        continue
    advisories = deny.get("advisories")
    if not isinstance(advisories, dict):
        fail_row("advisories", deny_subject, "[advisories] table missing")
        continue
    ignored = advisories.get("ignore")
    if not isinstance(ignored, list):
        fail_row("advisories", deny_subject,
                 "[advisories].ignore must be an empty array")
    elif ignored:
        for advisory in ignored:
            fail_row("advisories", f"{deny_subject}:{advisory}",
                     "ignored advisory must be a policy exception instead")
    else:
        pass_row("advisories", f"{deny_subject} ignore list", "empty")
if with_advisories:
    cargo_deny = shutil.which("cargo-deny")
    if cargo_deny is None:
        fail_row("advisories", "live scan",
                 "--with-advisories requested but cargo-deny is not on PATH")
    else:
        for workspace_relative, _ in workspace_roots:
            workspace_dir = os.path.join(root, workspace_relative)
            manifest_path = os.path.join(workspace_dir, "Cargo.toml")
            deny_path = os.path.join(workspace_dir, "deny.toml")
            subject = os.path.relpath(manifest_path, root)
            try:
                completed = subprocess.run(
                    [cargo_deny, "--locked", "--config", deny_path,
                     "--manifest-path", manifest_path, "check", "advisories"],
                    cwd=root, capture_output=True, text=True, timeout=180)
            except (OSError, subprocess.TimeoutExpired) as err:
                fail_row("advisories", subject, f"live scan failed ({err})")
            else:
                if completed.returncode == 0:
                    pass_row("advisories", subject,
                             "cargo deny check advisories: no findings")
                else:
                    tail = (completed.stdout + completed.stderr)[-500:]
                    fail_row("advisories", subject,
                             "cargo deny reported findings; run "
                             f"`cargo deny check advisories` for evidence: {tail!r}")
else:
    info_row("advisories", "live scan",
             "runs for every Cargo workspace in CI; --with-advisories runs it here")

# --- Bounded read-only upstream probe (weekly freshness.yml; writes nothing).
FETCH_TIMEOUT = 10
FETCH_CAP = 512 * 1024


def fetch_text(url):
    request = urllib.request.Request(
        url, headers={"User-Agent": "velnor-freshness-probe",
                      "Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=FETCH_TIMEOUT) as response:
        return response.read(FETCH_CAP + 1)[:FETCH_CAP + 1].decode(
            "utf-8", errors="replace")


def github_tag(payload):
    if isinstance(payload, dict) and payload.get("tag_name"):
        return payload["tag_name"]
    if isinstance(payload, list):
        for release in payload:
            if not isinstance(release, dict) or release.get("draft") \
                    or release.get("prerelease"):
                continue
            # Release lists carry `tag_name`; tag lists carry `name`.
            tag = release.get("tag_name") or release.get("name")
            if tag:
                return tag
    return None


def sniff_latest(source, body):
    try:
        payload = json.loads(body)
    except ValueError:
        payload = None
    if payload is not None:
        if "crates.io/api/v1/crates/" in source \
                and isinstance(payload, dict):
            crate = payload.get("crate") or {}
            return crate.get("max_version")
        tag = github_tag(payload)
        if tag:
            return tag
        crate = (payload.get("crate") or {}) if isinstance(payload, dict) \
            else {}
        if crate.get("max_version"):
            return crate["max_version"]
    match = re.search(r"\[pkg\.rust\]\s*\nversion\s*=\s*\""
                      r"(\d+\.\d+\.\d+)", body)
    if not match:
        match = re.search(r'version\s*=\s*"(\d+\.\d+\.\d+)', body)
    return match.group(1) if match else None


if check_upstream:
    stamp = NOW.strftime("%Y-%m-%dT%H:%M:%SZ")
    for tool in tools:
        name = tool.get("name")
        source = tool.get("source", "")
        try:
            latest = sniff_latest(source, fetch_text(source))
        except Exception as err:  # noqa: BLE001 - probe maps all to failed
            fail_row("upstream-probe", name,
                     f"lookup_failed ({err}); source {source}, "
                     f"checked {stamp}")
            continue
        if latest is None:
            fail_row("upstream-probe", name,
                     f"lookup_failed: no stable release parsed; source "
                     f"{source}, checked {stamp}")
        elif norm_version(latest) != norm_version(tool.get("pinned")):
            fail_row("upstream-probe", name,
                     f"stale pin: pinned={tool.get('pinned')!r} "
                     f"latest={latest!r}; source {source}, checked {stamp}")
        else:
            pass_row("upstream-probe", name,
                     f"pinned==latest {latest}; source {source}, "
                     f"checked {stamp}")
    for key, action in sorted(action_pinned.items()):
        source = action.get("source", "")
        pinned = action.get("pinned_version")
        try:
            latest = sniff_latest(source, fetch_text(source))
        except Exception as err:  # noqa: BLE001 - probe maps all to failed
            fail_row("upstream-probe", key,
                     f"lookup_failed ({err}); source {source}, "
                     f"checked {stamp}")
            continue
        if latest is None:
            fail_row("upstream-probe", key,
                     f"lookup_failed: no stable release parsed; source "
                     f"{source}, checked {stamp}")
        elif norm_version(latest) != norm_version(pinned):
            fail_row("upstream-probe", key,
                     f"stale pin: pinned={pinned!r} latest={latest!r}; "
                     f"source {source}, checked {stamp}")
        else:
            pass_row("upstream-probe", key,
                     f"pinned==latest {latest}; source {source}, "
                     f"checked {stamp}")
    info_row("upstream-probe", "runner",
             "latest image family is platform-qualification evidence, "
             "not an API probe")

if failures:
    print("check-freshness: FAIL", file=sys.stderr)
    for failure in failures:
        print(f"  - {failure}", file=sys.stderr)
    sys.exit(1)
print("check-freshness: PASS")
EOF

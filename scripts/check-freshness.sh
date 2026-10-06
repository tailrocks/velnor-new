#!/usr/bin/env bash
# Fail-closed freshness gate over the Velnor-owned version inventories.
#
# Validates four separated concerns (never conflated):
#   local-pin          compiled-in constants == reviewed inventory pins,
#                      including the version-policy mirror and the
#                      cargo-mutants activation pin.
#   effective-identity declared manifest requirements == Cargo.lock
#                      resolution, keyed by name+version+source with every
#                      dependency form and scope covered.
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
#   --with-advisories  run the live `cargo deny check advisories` scan
#                      (180 s timeout) in addition to the deny-policy checks.
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


def rust_without_comments(text, subject="mise compiled qualification"):
    result = []
    index = 0
    while index < len(text):
        if text[index] == '"':
            start = index
            index += 1
            while index < len(text):
                if text[index] == "\\":
                    index += 2
                elif text[index] == '"':
                    index += 1
                    break
                else:
                    index += 1
            else:
                fail_row("local-pin", subject, "unterminated source string")
                return None
            result.append(text[start:index])
        elif text.startswith("//", index):
            end = text.find("\n", index)
            index = len(text) if end == -1 else end
            result.append(" ")
        elif text.startswith("/*", index):
            depth = 1
            index += 2
            while index < len(text) and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            if depth:
                fail_row("local-pin", subject, "unterminated source comment")
                return None
            result.append(" ")
        else:
            result.append(text[index])
            index += 1
    return "".join(result)



def rust_source(path):
    subject = f"{path} (compiled source)"
    try:
        with open(f"{root}/{path}", encoding="utf-8") as handle:
            return rust_without_comments(handle.read(), subject)
    except OSError as err:
        fail_row("local-pin", subject, f"unreadable ({err})")
        return None


def authority_symbol_supported(text, symbol, declaration_start, path):
    """The supported authority grammar excludes conditional/rebound symbols."""
    subject = f"{path}::{symbol}"
    if text[:declaration_start].rstrip().endswith("]") or "#![" in text:
        fail_row("local-pin", subject, "unsupported authority attribute")
        return False
    for imported in re.findall(r"\buse\s+([^;]+);", text, re.S):
        if "*" in imported or re.search(rf"\b{re.escape(symbol)}\b", imported):
            fail_row("local-pin", subject, "unsupported authority import binding")
            return False
    return True


def rust_const_expression(path, name, visibility="pub", strict_authority=False):
    """Require one supported live declaration, never a commented mirror."""
    text = rust_source(path)
    if text is None:
        return None
    declarations = list(re.finditer(
        rf"(?<!\w)(?:pub(?:\([^)]*\))?\s+)?const\s+{re.escape(name)}\b[^;]*;",
        text, re.M))
    expected = rf'\s*{re.escape(visibility)}\s+const\s+{re.escape(name)}:\s*&str\s*=\s*(.+);'
    if len(declarations) != 1:
        fail_row("local-pin", f"{path}::{name}", f"missing or duplicate const {name}")
        return None
    if strict_authority and not authority_symbol_supported(text, name, declarations[0].start(), path):
        return None
    match = re.fullmatch(expected, declarations[0].group(0), re.S)
    if match is None:
        fail_row("local-pin", f"{path}::{name}", f"unsupported const {name} declaration")
        return None
    return match.group(1).strip()


def rust_const(path, name, visibility="pub", strict_authority=False):
    """Extract one exact source literal with its required visibility."""
    expression = rust_const_expression(path, name, visibility, strict_authority)
    if expression is None:
        return None
    match = re.fullmatch(r'"([^"\\]*)"', expression)
    if match is None:
        fail_row("local-pin", f"{path}::{name}", f"unsupported literal const {name}")
        return None
    return match.group(1)


def authority_module_supported(path, module, filename, visibility=""):
    source = rust_source(path)
    if source is None:
        return False
    declarations = re.findall(rf"(?<!\w)(?:pub\s+)?mod\s+{module}\b[^;]*;", source, re.M)
    bindings = list(re.finditer(
        rf'#\[path\s*=\s*"{re.escape(filename)}"\]\s*{visibility}mod\s+{module}\s*;', source))
    if len(declarations) != 1 or len(bindings) != 1:
        fail_row("local-pin", f"{path}::{module}", "missing, duplicate, or misaligned native module binding")
        return False
    return authority_symbol_supported(source, module, bindings[0].start(), path)


def profile_version(name):
    """Resolve only the three explicit compiled selection authority chains."""
    qualification = "crates/velnor-actions-mise/src/catalog_qualification.rs"
    profiles = {
        "java": (WORKLOAD_CATALOG, "JAVA", "super::qualification::", "java"),
        "gradle": (WORKLOAD_CATALOG, "GRADLE", "super::qualification::", "gradle"),
        "cargo-semver-checks": ("crates/velnor-actions-mise/src/catalog.rs",
                               "CARGO_SEMVER_CHECKS", "qualification::", "semver"),
    }
    catalog, prefix, namespace, record = profiles[name]
    const = f"{prefix}_VERSION"
    selection = f"{prefix}_SELECTION_VERSION"
    module = f"{record}_records"
    filename = f"catalog_qualification_{record}.rs"
    authority = f"crates/velnor-actions-mise/src/{filename}"
    for path, symbol, expected in (
            (catalog, const, f"{namespace}{selection}"),
            (qualification, selection, f"{module}::VERSION")):
        expression = rust_const_expression(path, symbol, strict_authority=True)
        if expression is None:
            return None
        if expression != expected:
            fail_row("local-pin", f"{path}::{symbol}", f"unsupported selection alias: expected {expected}")
            return None
    if name == "cargo-semver-checks" and not authority_module_supported(
            catalog, "qualification", "catalog_qualification.rs", r"pub\s+"):
        return None
    if not authority_module_supported(qualification, module, filename):
        return None
    return rust_const(authority, "VERSION", "pub(super)", strict_authority=True)



def parse_iso_date(text):
    """Strict `YYYY-MM-DD` date or None."""
    if not isinstance(text, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", text):
        return None
    try:
        return datetime.date.fromisoformat(text)
    except ValueError:
        return None


def norm_version(text):
    """Compare numeric pins with the catalog's exact upstream tag forms."""
    version = (text or "").strip()
    for prefix in ("bun-v", "swift-", "cargo-nextest-", "jq-", "v"):
        if version.startswith(prefix):
            version = version.removeprefix(prefix)
            break
    return version.removesuffix("-RELEASE").split("+", 1)[0]


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
                   "temporary_holds", "delivery_tools", "native_authorities"):
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
                   "validation-tools", "delivery-tools", "native-authorities"):
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
CATALOG = "crates/velnor-actions-mise/src/catalog_pins.rs"
WORKLOAD_CATALOG = "crates/velnor-actions-mise/src/catalog_workloads.rs"
DESKTOP_CATALOG = "crates/velnor-actions-mise/src/catalog_rust_desktop.rs"
ACTIONS = "crates/velnor-actions-actionlint/src/actions.rs"
TOOLS = "crates/velnor-actions-actionlint/src/tools.rs"
CAPABILITIES = "crates/velnor-actions-actionlint/src/capabilities.rs"
CONFIG = "crates/velnor-actions-actionlint/src/config.rs"
RENDERER = "crates/velnor-actions-workflow-renderer/src/render.rs"

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
    "cargo-semver-checks": "CARGO_SEMVER_CHECKS_VERSION",
    "bun": "BUN_VERSION",
    "swift": "SWIFT_VERSION",
    "ruby": "RUBY_VERSION",
    "reuse": "REUSE_VERSION",
    "java": "JAVA_VERSION",
    "gradle": "GRADLE_VERSION",
    "python": "PYTHON_VERSION",
    "uv": "UV_VERSION",
    "cargo-audit": "CARGO_AUDIT_VERSION",
    "cargo-deny": "CARGO_DENY_VERSION",
    "alint": "ALINT_VERSION",
    "node": "NODE_VERSION",
    "boltffi": "BOLTFFI_VERSION",
    "xcodegen": "XCODEGEN_VERSION",
    "jq": "JQ_VERSION",
    "rust-desktop": "DESKTOP_RUST_VERSION",
    "swiftlint": "SWIFTLINT_VERSION",
    "periphery": "PERIPHERY_VERSION",
}
WORKLOAD_TOOLS = {"bun", "swift", "ruby", "reuse", "java", "gradle", "python", "uv",
                  "cargo-audit", "cargo-deny", "alint", "node", "boltffi", "xcodegen", "jq",
                  "swiftlint", "periphery"}
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
    "docker/setup-buildx-action": "SETUP_BUILDX_ACTION",
    "docker/login-action": "LOGIN_ACTION",
    "docker/build-push-action": "BUILD_PUSH_ACTION",
    "actions/configure-pages": "CONFIGURE_PAGES_ACTION",
    "actions/upload-pages-artifact": "UPLOAD_PAGES_ARTIFACT_ACTION",
    "actions/deploy-pages": "DEPLOY_PAGES_ACTION",
    "actions/attest": "ATTEST_ACTION",
}
EXPECTED_DELIVERY_TOOLS = {
    "buildx": ("BUILDX_VERSION", None),
    "buildkit": ("BUILDKIT_VERSION", "BUILDKIT_IMAGE_DIGEST"),
    "sbom-scanner": ("SBOM_SCANNER_VERSION", "SBOM_SCANNER_IMAGE_DIGEST"),
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
    if name in seen_tools:
        fail_row("inventory-shape", str(name), "duplicate tool")
    seen_tools.add(name)
    const = EXPECTED_TOOLS.get(name)
    if const is None:
        fail_row("local-pin", f"tool {name}",
                 "inventory entry outside the expected tool set")
        continue
    source = ("crates/velnor-actions-mise/src/catalog.rs" if name == "cargo-semver-checks" else
              DESKTOP_CATALOG if name == "rust-desktop" else
              WORKLOAD_CATALOG if name in WORKLOAD_TOOLS else CATALOG)
    pin_row(f"tool {name} ({source}::{const})",
            (profile_version(name) if name in ("java", "gradle", "cargo-semver-checks") else
             rust_const(source, const)), tool.get("pinned"))
for name in sorted(set(EXPECTED_TOOLS) - seen_tools):
    fail_row("local-pin", f"tool {name}", "inventory row missing")

MISE_QUALIFICATIONS = "crates/velnor-actions-mise/src/catalog_qualification_records.rs"
MISE_ARTIFACTS = {
    "x86_64-unknown-linux-gnu": ("LinuxAmd64", "linux-x64"),
    "aarch64-unknown-linux-gnu": ("LinuxArm64", "linux-arm64.tar.gz"),
    "aarch64-apple-darwin": ("MacosArm64", "macos-arm64.tar.gz"),
}



def mise_constructor_identity(constructor):
    expected = {name: name for name in ("tool", "host", "asset_format")}
    expected.update({name: f"{name}: asset.{name}()" for name in (
        "selector", "asset_url", "archive_sha256", "binary_sha256", "binary_member",
        "source_repository", "source_commit", "source_tree", "owner", "version", "abi")})
    expected.update({
        "selection_version": "selection_version: asset.version()",
        "installed_binary_relative_path": "installed_binary_relative_path: None",
        "launch_entries": "launch_entries: &[]",
        "source_lineage": "source_lineage: &[]",
        "install_plan": "install_plan: None",
        "provisioning_mode": "provisioning_mode: ProvisioningMode::Official",
    })
    found = {}
    for value in constructor.split(","):
        value = value.strip()
        if not value:
            continue
        match = re.fullmatch(r"(\w+)(?:\s*:\s*(.+))?", value, re.S)
        if match is None or match.group(1) in found:
            break
        name = match.group(1)
        found[name] = name if match.group(2) is None else f"{name}: {match.group(2).strip()}"
    else:
        if found == expected:
            return True
    fail_row("local-pin", "mise compiled qualification", "unsupported official qualification constructor identity")
    return False


def mise_source_constructor_identity(constructor, source):
    expected = {name: name for name in (
        "host", "asset_url", "archive_sha256", "binary_sha256", "asset_format", "binary_member")}
    expected.update({
        "tool": "tool: SourceBuildBootstrapTool::Mise",
        "selector": f'selector: "github:jdx/mise@{source["version"]}"',
        "source_repository": 'source_repository: "https://github.com/jdx/mise"',
        "source_commit": f'source_commit: "{source["source_commit"]}"',
        "source_tree": f'source_tree: "{source["source_tree"]}"',
        "owner": 'owner: "jdx/mise"',
        "version": f'version: "{source["version"]}"',
        "abi": f'abi: "mise-cli-v{source["version"]}"',
    })
    values = [value.strip() for value in constructor.split(",") if value.strip()]
    if len(values) == len(expected) and set(values) == set(expected.values()):
        return True
    fail_row("local-pin", "mise compiled qualification", "unsupported source bootstrap constructor identity")
    return False


def mise_qualification_records():
    subject = "mise compiled qualification"
    source_path = "crates/velnor-actions-mise/src/catalog_source_build_bootstrap.rs"
    if not authority_module_supported("crates/velnor-actions-mise/src/catalog.rs",
            "source_build_bootstrap", "catalog_source_build_bootstrap.rs", r"pub\s+") or not authority_module_supported(
            "crates/velnor-actions-mise/src/catalog_qualification.rs", "records", "catalog_qualification_records.rs"):
        return None
    adapter = rust_source(MISE_QUALIFICATIONS)
    text = rust_source(source_path)
    if adapter is None or text is None:
        return None
    expected_imports = ['super::super::source_build_bootstrap::{self,SourceBuildBootstrapFormat,SourceBuildBootstrapHost,SourceBuildBootstrapTool,}', 'super::{DistributionAssetFormat,DistributionHost,DistributionTool,ProvisioningMode,QualifiedDistribution,}', 'crate::MiseError']
    if sorted("".join(value.split()) for value in re.findall(r"\buse\s+([^;]+);", adapter, re.S)) != sorted(expected_imports) or "#[" in adapter or "#![" in adapter:
        fail_row("local-pin", subject, "unsupported official qualification source binding")
        return None
    supported = re.sub(r'#\[cfg\(test\)\]\s*#\[path\s*=\s*"catalog_source_build_bootstrap_tests.rs"\]\s*mod tests;', "", text)
    supported = supported.replace("#[must_use]", "").replace("#[derive(Debug, Clone, Copy, PartialEq, Eq)]", "")
    if "#[" in supported or "#![" in supported or re.search(r"\buse\b", supported):
        fail_row("local-pin", subject, "unsupported source bootstrap authority binding")
        return None
    functions = list(re.finditer(r"\bfn\s+official\s*\(", adapter))
    official = re.search(r"pub\(super\) fn official\(.+?(?=pub\(super\) fn required)", adapter, re.S)
    if len(functions) != 1 or official is None:
        fail_row("local-pin", subject, "missing or duplicate official qualification function")
        return None
    body = official.group(0)
    prefix, separator, constructor = body.partition("QualifiedDistribution {")
    expected = 'pub(super)fnofficial(tool:DistributionTool,host:DistributionHost,)->Result<QualifiedDistribution,MiseError>{letsource_tool=matchtool{DistributionTool::Mise=>SourceBuildBootstrapTool::Mise,DistributionTool::Mbx=>SourceBuildBootstrapTool::Mbx,_=>returnErr(absent(tool,host,"officialinstalled-bytequalification")),};letsource_host=matchhost{DistributionHost::LinuxAmd64=>SourceBuildBootstrapHost::LinuxAmd64,DistributionHost::LinuxArm64=>SourceBuildBootstrapHost::LinuxArm64,DistributionHost::MacosArm64=>SourceBuildBootstrapHost::MacosArm64,};letasset=source_build_bootstrap::official(source_tool,source_host);letasset_format=matchasset.asset_format(){SourceBuildBootstrapFormat::Binary=>DistributionAssetFormat::Binary,SourceBuildBootstrapFormat::TarGzip=>DistributionAssetFormat::TarGzip,};'
    if "".join(prefix.split()) != expected or not separator:
        fail_row("local-pin", subject, "unsupported official qualification source adapter")
        return None
    constructors = re.findall(r"QualifiedDistribution\s*\{([^{}]+)\}\s*\.validate\(\)\s*\}\s*$", body)
    if len(constructors) != 1:
        fail_row("local-pin", subject, "unsupported official qualification constructor shape")
        return None
    if not mise_constructor_identity(constructors[0]):
        return None
    dispatch = re.findall(r"pub const fn official\(\s*tool:\s*SourceBuildBootstrapTool\s*,\s*host:\s*SourceBuildBootstrapHost\s*,?\s*\)\s*->\s*SourceBuildBootstrapAsset\s*\{([^{}]+)\{([^{}]+)\}\s*\}", text)
    expected_dispatch = "SourceBuildBootstrapTool::Mise=>mise(host),SourceBuildBootstrapTool::Mbx=>mbx(host),"
    if len(re.findall(r"\bfn\s+official\s*\(", text)) != 1 or len(dispatch) != 1 or "".join(dispatch[0][0].split()) != "matchtool" or "".join(dispatch[0][1].split()) != expected_dispatch:
        fail_row("local-pin", subject, "unsupported source bootstrap dispatch")
        return None
    for field in ("selector", "asset_url", "archive_sha256", "binary_sha256", "asset_format",
                  "binary_member", "source_repository", "source_commit", "source_tree", "owner", "version", "abi"):
        getters = re.findall(rf"pub const fn {field}\(&self\)\s*->[^{{]+\{{([^{{}}]+)\}}", text)
        if len(getters) != 1 or getters[0].strip() != f"self.{field}":
            fail_row("local-pin", subject, "unsupported source bootstrap accessor identity")
            return None
    functions = list(re.finditer(r"\bfn\s+mise\s*\(", text))
    official = re.search(r"const fn mise\(.+?(?=const fn mbx)", text, re.S)
    if len(functions) != 1 or official is None:
        fail_row("local-pin", subject, "missing or duplicate source bootstrap function")
        return None
    body = official.group(0)
    guard = re.match(r"const\s+fn\s+mise\(\s*host:\s*SourceBuildBootstrapHost\s*\)\s*->\s*SourceBuildBootstrapAsset\s*\{\s*(let\b.+)", body, re.S)
    if guard is None:
        fail_row("local-pin", subject, "unsupported source bootstrap function")
        return None
    body = guard.group(1)
    matches = re.findall(r"\Alet\s*\(\s*asset_url\s*,\s*archive_sha256\s*,\s*binary_sha256\s*,\s*asset_format\s*,\s*binary_member\s*\)\s*=\s*match\s+host\s*\{([^{}]+)\};\s*SourceBuildBootstrapAsset", body)
    if len(matches) != 1:
        fail_row("local-pin", subject, "tuple fields must forward unchanged from supported host match")
        return None
    tuple_pattern = r'SourceBuildBootstrapHost::(\w+)\s*=>\s*\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*SourceBuildBootstrapFormat::(\w+)\s*,\s*"([^"]*)"\s*,?\s*\)'
    tuples = re.findall(tuple_pattern, matches[0])
    if re.sub(tuple_pattern, "", matches[0]).strip(" \t\r\n,"):
        fail_row("local-pin", subject, "unsupported qualified host match arm")
        return None
    hosts = [values[0] for values in tuples]
    arms = re.findall(r"SourceBuildBootstrapHost::(\w+)\s*=>", matches[0])
    if hosts != arms or len(hosts) != len(set(hosts)) or set(hosts) != {host for host, _ in MISE_ARTIFACTS.values()}:
        fail_row("local-pin", subject, "missing, duplicate, or unknown qualified host")
        return None
    constructors = re.findall(r"SourceBuildBootstrapAsset\s*\{([^{}]+)\}\s*\}\s*$", body)
    if len(constructors) != 1 or any(len(re.findall(
            rf"(?:^|,)\s*{field}\s*(?=,|$)", constructors[0])) != 1
            for field in ("asset_url", "archive_sha256", "binary_sha256", "asset_format", "binary_member")):
        fail_row("local-pin", subject, "tuple fields must forward unchanged in supported qualification constructor")
        return None
    fields = {}
    for field in ("source_repository", "source_commit", "source_tree", "version"):
        values = re.findall(rf'(?:^|,)\s*{field}:\s*"([^"]+)"\s*(?=,|$)', constructors[0])
        if len(values) != 1:
            fail_row("local-pin", subject, f"missing or duplicate source field {field}")
            return None
        fields[field] = values[0]
    if not mise_source_constructor_identity(constructors[0], fields):
        return None
    if fields["source_repository"] != "https://github.com/jdx/mise" or any(
            not re.fullmatch(r"[0-9a-f]{40}", fields[key]) for key in ("source_commit", "source_tree")):
        fail_row("local-pin", subject, "invalid compiled source identity")
        return None
    records = {host: (url, archive, binary, format_name, member)
               for host, url, archive, binary, format_name, member in tuples}
    if any(not re.fullmatch(r"[0-9a-f]{64}", digest)
           for _, archive, binary, _, _ in records.values() for digest in (archive, binary)):
        fail_row("local-pin", subject, "malformed compiled qualification digest")
        return None
    for host, (_, archive, binary, format_name, member) in records.items():
        expected = ("Binary", "") if host == "LinuxAmd64" else ("TarGzip", "mise/bin/mise")
        if (format_name, member) != expected or (format_name == "Binary" and archive != binary):
            fail_row("local-pin", subject, "unsupported compiled qualification container or member")
            return None
    return (fields, records)


def verify_mise_artifact(entry, mise, spec, qualification):
    target = entry["target"]
    subject = f"mise binary {target}"
    before = len(failures)
    host, suffix = spec
    source, records = qualification
    asset_url, archive_digest, binary_digest, format_name, member = records[host]
    digest = entry.get("installed_binary_sha256")
    qualified = entry.get("qualified_installed_binary_sha256")
    if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
        fail_row("inventory-shape", subject, "malformed installed binary SHA256")
    pin_row(subject, binary_digest, digest)
    if qualified != digest:
        fail_row("source-evidence", subject, "installed binary digest has no matching qualification")
    archive = entry.get("archive_sha256", digest if format_name == "Binary" else None)
    if archive != archive_digest:
        fail_row("local-pin", subject, "archive digest differs from compiled qualification")
    commit = entry.get("source_commit")
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit) \
            or commit != mise.get("source_commit_sha") or commit != source["source_commit"]:
        fail_row("source-evidence", subject, "artifact source commit differs from Mise source identity")
    if entry.get("source_tree") != source["source_tree"] or mise.get("source_tree_sha") != source["source_tree"]:
        fail_row("source-evidence", subject, "artifact source tree differs from compiled source identity")
    version = mise.get("pinned")
    url = f"https://github.com/jdx/mise/releases/download/v{version}/mise-v{version}-{suffix}"
    if entry.get("artifact") != url or asset_url != url or version != source["version"]:
        fail_row("source-evidence", subject, "artifact URL differs from pinned official release asset")
    if entry.get("archive_member", "") != member:
        fail_row("source-evidence", subject, "archive member must identify installed Mise executable")
    if entry.get("asset_format") != {"Binary": "binary", "TarGzip": "tar-gzip"}[format_name]:
        fail_row("source-evidence", subject, "asset format differs from compiled qualification container")
    if entry.get("binary_member") != member:
        fail_row("source-evidence", subject, "binary member differs from compiled qualification member")
    if len(failures) == before:
        pass_row("source-evidence", subject, "qualified installed executable source identity")


def verify_mise_artifacts():
    qualification = mise_qualification_records()
    if qualification is None:
        return
    matches = [tool for tool in tools if isinstance(tool, dict) and tool.get("name") == "mise"]
    if len(matches) != 1:
        fail_row("inventory-shape", "mise native artifacts", "requires exactly one Mise tool row")
        return
    mise = matches[0]
    artifacts = mise.get("native_artifacts")
    if not isinstance(artifacts, list):
        fail_row("inventory-shape", "mise native artifacts", "must be a list")
        return
    seen = set()
    for entry in artifacts:
        if not isinstance(entry, dict) or not isinstance(entry.get("target"), str):
            fail_row("inventory-shape", "mise native artifacts", "malformed artifact record")
            continue
        target = entry["target"]
        if target not in MISE_ARTIFACTS or target in seen:
            fail_row("inventory-shape", "mise native artifacts", f"unknown or duplicate target {target!r}")
            continue
        seen.add(target)
        verify_mise_artifact(entry, mise, MISE_ARTIFACTS[target], qualification)
    for target in sorted(set(MISE_ARTIFACTS) - seen):
        fail_row("local-pin", f"mise binary {target}", "artifact inventory row missing")


verify_mise_artifacts()

action_pinned = {}
for action in inv.get("actions", []):
    if not isinstance(action, dict):
        fail_row("inventory-shape", "(inventory actions)",
                 f"entry must be an object, got {action!r}")
        continue
    key = action.get("key")
    if key in action_pinned:
        fail_row("inventory-shape", str(key), "duplicate action")
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

delivery_tools = inv.get("delivery_tools", [])
if not isinstance(delivery_tools, list):
    fail_row("inventory-shape", "delivery tools", "must be a list")
    delivery_tools = []
delivery_pinned = {}
for tool in delivery_tools:
    if not isinstance(tool, dict) or tool.get("name") not in EXPECTED_DELIVERY_TOOLS:
        fail_row("inventory-shape", "delivery tools", "unknown or malformed entry")
        continue
    name = tool["name"]
    if name in delivery_pinned:
        fail_row("inventory-shape", name, "duplicate delivery tool")
    delivery_pinned[name] = tool
    version_const, digest_const = EXPECTED_DELIVERY_TOOLS[name]
    pin_row(f"delivery tool {name}", rust_const(ACTIONS, version_const), tool.get("pinned"))
    if digest_const:
        pin_row(f"delivery image {name}", rust_const(ACTIONS, digest_const), tool.get("image_digest"))
        if tool.get("qualified_image_digest") != tool.get("image_digest"):
            fail_row("local-pin", name, "delivery image digest has no matching qualification")
for name in sorted(set(EXPECTED_DELIVERY_TOOLS) - set(delivery_pinned)):
    fail_row("local-pin", name, "delivery inventory row missing")

# Native authorities prove their scoped immutable sources, never latest releases.
HOMEBREW = "crates/velnor-actions-mise/src/catalog_homebrew.rs"
GRADLE_AUTHORITY = "crates/velnor-actions-mise/src/catalog_gradle.rs"
NATIVE_AUTHORITIES = {
    "homebrew-source": ("homebrew-native-source", HOMEBREW, {
        "homebrew-version": ("VERSION", r"\d+\.\d+\.\d+"),
        "homebrew-source-sha": ("SOURCE_SHA", r"[0-9a-f]{40}"),
        "homebrew-source-url": ("SOURCE_URL", r"https://github\.com/Homebrew/brew/tree/[0-9a-f]{40}"),
    }),
    "homebrew-portable-ruby": ("homebrew-vendored-ruby", HOMEBREW, {
        "homebrew-portable-ruby-version": ("PORTABLE_RUBY_VERSION", r"\d+\.\d+\.\d+"),
        "homebrew-portable-ruby-x86_64-linux-sha256": ("PORTABLE_RUBY_X86_64_LINUX_SHA256", r"[0-9a-f]{64}"),
        "homebrew-portable-ruby-arm64-linux-sha256": ("PORTABLE_RUBY_ARM64_LINUX_SHA256", r"[0-9a-f]{64}"),
    }),
    "gradle-wrapper": ("gradle-workload-wrapper", GRADLE_AUTHORITY, {
        "gradle-wrapper-version": ("GRADLE_WRAPPER_VERSION", r"\d+\.\d+\.\d+"),
        "gradle-wrapper-distribution-sha256": ("GRADLE_WRAPPER_DISTRIBUTION_SHA256", r"[0-9a-f]{64}"),
        "gradle-wrapper-script-sha256": ("GRADLE_WRAPPER_SCRIPT_SHA256", r"[0-9a-f]{64}"),
        "gradle-wrapper-jar-sha256": ("GRADLE_WRAPPER_JAR_SHA256", r"[0-9a-f]{64}"),
        "gradle-wrapper-bootstrap-version": ("BOOTSTRAP_VERSION", r"\d+\.\d+\.\d+"),
        "gradle-wrapper-bootstrap-source-sha": ("BOOTSTRAP_SOURCE_SHA", r"[0-9a-f]{40}"),
    }),
    "postgres-fixture": ("gradle-postgres-fixture", GRADLE_AUTHORITY, {
        "postgres-fixture-image": ("POSTGRES_FIXTURE_IMAGE", r"postgres:\d+\.\d+-trixie@sha256:[0-9a-f]{64}"),
    }),
}


def native_authority_sources(name, compiled):
    if name.startswith("homebrew-"):
        sha = rust_const(HOMEBREW, "SOURCE_SHA")
        if sha is None:
            return None
        source_url = f"https://github.com/Homebrew/brew/tree/{sha}"
        if name == "homebrew-source":
            if compiled["homebrew-source-url"] != source_url:
                fail_row("source-evidence", name, "source URL does not bind source commit")
                return None
            return [source_url]
        return [f"https://raw.githubusercontent.com/Homebrew/brew/{sha}/Library/Homebrew/vendor/{file}"
                for file in ("portable-ruby-version", "portable-ruby-x86_64-linux", "portable-ruby-arm64-linux")]
    if name == "postgres-fixture":
        digest = compiled["postgres-fixture-image"].split("@", 1)[1]
        return [f"https://registry-1.docker.io/v2/library/postgres/manifests/{digest}"]
    sha = compiled["gradle-wrapper-bootstrap-source-sha"]
    version = compiled["gradle-wrapper-version"]
    expected = {
        "BOOTSTRAP_TEMPLATE_SOURCE": f"https://github.com/gradle/gradle/blob/{sha}/platforms/jvm/plugins-application/src/main/resources/org/gradle/api/internal/plugins/unixStartScript.txt",
        "BOOTSTRAP_JAR_SOURCE": f"https://raw.githubusercontent.com/gradle/gradle/{sha}/gradle/wrapper/gradle-wrapper.jar",
        "DISTRIBUTION_CHECKSUM_SOURCE": f"https://services.gradle.org/distributions/gradle-{version}-bin.zip.sha256",
    }
    before = len(failures)
    for constant, source in expected.items():
        actual = rust_const(GRADLE_AUTHORITY, constant)
        if actual is not None and actual != source:
            fail_row("source-evidence", name, f"{constant} does not bind compiled source commit or engine version")
    return list(expected.values()) if len(failures) == before else None


def native_authority_rows():
    entries = inv.get("native_authorities")
    if not isinstance(entries, list):
        fail_row("inventory-shape", "native authorities", "must be a list")
        return {}
    rows = {}
    fields = {"name", "scope", "pins", "evidence_kind", "sources", "checked_at"}
    for entry in entries:
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            fail_row("inventory-shape", "native authorities", "malformed entry")
            continue
        name = entry["name"]
        if name not in NATIVE_AUTHORITIES or set(entry) != fields or name in rows:
            fail_row("inventory-shape", name, "unknown, duplicate, or invalid native fields")
            continue
        rows[name] = entry
    return rows


def verify_native_authority(name, spec, entry, native_policy):
    scope, path, bindings = spec
    subject = f"native {name}"
    before = len(failures)
    if entry is None:
        fail_row("local-pin", subject, "inventory row missing")
        return
    pins = entry.get("pins")
    if not isinstance(pins, dict) or set(pins) != set(bindings):
        fail_row("inventory-shape", subject, "missing or unknown native pins")
        return
    compiled = {}
    for key, (constant, pattern) in bindings.items():
        value = rust_const(path, constant)
        compiled[key] = value
        if not isinstance(pins[key], str) or not re.fullmatch(pattern, pins[key]):
            fail_row("inventory-shape", key, "malformed native pin")
        pin_row(subject + " " + key, value, pins[key])
        if native_policy.get(key) != pins[key]:
            fail_row("policy-mirror", key, "native policy/inventory mismatch")
        else:
            pass_row("policy-mirror", key, pins[key])
    if len(failures) != before:
        return
    sources = native_authority_sources(name, compiled)
    if sources is None:
        return
    evidence_sources = entry.get("sources")
    if entry.get("scope") != scope or entry.get("evidence_kind") != "immutable-source":
        fail_row("source-evidence", subject, "invalid immutable authority scope or evidence kind")
    elif not isinstance(evidence_sources, list) or any(not isinstance(url, str) for url in evidence_sources) \
            or sorted(evidence_sources) != sorted(sources):
        fail_row("source-evidence", subject, "sources do not match immutable compiled authority")
    else:
        stamp = entry.get("checked_at")
        moment = parse_timestamp(stamp) if isinstance(stamp, str) and re.fullmatch(
            r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})", stamp) else None
        age = (NOW - moment).total_seconds() / 3600 if moment is not None else None
        if age is None or age < -0.1 or age > interval:
            fail_row("source-evidence", subject, "missing, malformed, future, or stale evidence timestamp")
        else:
            pass_row("source-evidence", subject, f"qualified immutable authority, evidence {entry['checked_at']}")


native_policy = policy.get("native-authorities") if policy is not None else None
native_keys = {key for _, _, bindings in NATIVE_AUTHORITIES.values() for key in bindings}
if not isinstance(native_policy, dict) or set(native_policy) != native_keys:
    fail_row("policy-mirror", "native authorities", "missing or unknown policy keys")
    native_policy = native_policy if isinstance(native_policy, dict) else {}
native_rows = native_authority_rows()
for name, spec in NATIVE_AUTHORITIES.items():
    verify_native_authority(name, spec, native_rows.get(name), native_policy)

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
    delivery_policy = policy.get("delivery-tools", {})
    if not isinstance(delivery_policy, dict):
        fail_row("policy-mirror", "delivery tools", "must be a table")
        delivery_policy = {}
    expected_delivery_keys = set(EXPECTED_DELIVERY_TOOLS)
    for name, (_, digest_const) in EXPECTED_DELIVERY_TOOLS.items():
        tool = delivery_pinned.get(name, {})
        pin_row(f"delivery policy {name}", delivery_policy.get(name), tool.get("pinned"))
        if digest_const:
            key = f"{name}-image-digest"
            expected_delivery_keys.add(key)
            pin_row(f"delivery policy {key}", delivery_policy.get(key), tool.get("image_digest"))
    if set(delivery_policy) != expected_delivery_keys:
        fail_row("policy-mirror", "delivery tools", "missing or unknown policy keys")
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
# Covers every dependency form (string, table, workspace-inherited,
# renamed via `package`) and scope (dependencies, dev-dependencies,
# build-dependencies, target.*). Lock entries are retained as a list so
# multiple versions of one name never shadow each other.
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


try:
    with open(f"{root}/Cargo.toml", "rb") as handle:
        workspace_doc = tomllib.load(handle)
    inherited = (workspace_doc.get("workspace") or {}).get("dependencies", {})
except (OSError, tomllib.TOMLDecodeError) as err:
    fail_row("lock-staleness", "workspace Cargo.toml", f"unreadable ({err})")
    inherited = None
try:
    with open(f"{root}/Cargo.lock", "rb") as handle:
        locked = tomllib.load(handle).get("package", [])
except (OSError, tomllib.TOMLDecodeError, AttributeError) as err:
    fail_row("lock-staleness", "Cargo.lock", f"unreadable ({err})")
    locked = None
if locked is not None and inherited is not None:
    manifests = sorted(globmod.glob(f"{root}/crates/*/Cargo.toml"))
    if not manifests:
        fail_row("lock-staleness", "crates/*/Cargo.toml", "no members found")
    member_names = set()
    declared = []  # (crate, scope, alias, real, req, detail)
    for manifest in manifests:
        try:
            with open(manifest, "rb") as handle:
                doc = tomllib.load(handle)
        except (OSError, tomllib.TOMLDecodeError) as err:
            fail_row("lock-staleness", manifest, f"unreadable ({err})")
            continue
        crate = doc.get("package", {}).get("name", manifest)
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
        # Every direct external requirement MUST be exact `=x.y.z` (VER-2.26).
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
    info_row("lock-staleness", "(declared-summary)",
             f"{passed} match, {failed} fail, "
             f"{len(by_name)} locked names retained")
    # Reverse direction: every locked package must trace to the workspace.
    local_names = {entry.get("name") for entry in locked
                   if not entry.get("source")}
    if local_names != member_names:
        fail_row("lock-staleness", "(lock-membership)",
                 f"local lock {sorted(local_names)} != "
                 f"members {sorted(member_names)}")
    else:
        pass_row("lock-staleness", "(lock-membership)",
                 f"{len(member_names)} members")
    queue = [entry for name in sorted(local_names & member_names)
             for entry in by_name.get(name, [])]
    reachable = {lock_identity(entry) for entry in queue}
    while queue:
        entry = queue.pop()
        for edge in entry.get("dependencies", []) or []:
            parts = edge.split(" ")
            cands = by_name.get(parts[0], [])
            if len(parts) > 1:
                cands = [c for c in cands
                         if (c.get("version") or "") == parts[1]]
            if len(parts) > 2:
                want_src = parts[2].strip("()")
                cands = [c for c in cands
                         if (c.get("source") or "") == want_src]
            if not cands:
                fail_row("lock-staleness", "(lock-graph)",
                         f"dangling edge {entry.get('name')} -> {edge}")
                continue
            for cand in cands:
                ident = lock_identity(cand)
                if ident not in reachable:
                    reachable.add(ident)
                    queue.append(cand)
    stranded = [entry for entry in locked
                if lock_identity(entry) not in reachable]
    if stranded:
        for entry in sorted(stranded, key=lambda e: e.get("name", "")):
            fail_row("lock-staleness", "(lock-graph)",
                     f"unreachable locked package "
                     f"{entry.get('name')} {entry.get('version')}")
    else:
        pass_row("lock-staleness", "(lock-graph)",
                 f"{len(locked)} locked packages reachable")
    try:
        lock_mtime = os.path.getmtime(f"{root}/Cargo.lock")
        newest_manifest = max(os.path.getmtime(m) for m in manifests)
        stale = lock_mtime < newest_manifest
        info_row("lock-mtime", "Cargo.lock",
                 f"lock_is_newest={str(not stale).lower()}")
    except OSError as err:
        info_row("lock-mtime", "Cargo.lock", f"mtime unreadable ({err})")

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
    if age < -0.1:
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
for tool in delivery_tools:
    if isinstance(tool, dict):
        freshness_row(tool.get("name"), tool, tool.get("pinned"), tool.get("qualified"), tool.get("latest"))
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
known_subjects = set(EXPECTED_TOOLS) | set(EXPECTED_ACTIONS) | set(EXPECTED_DELIVERY_TOOLS) | \
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

# --- Advisories: deny policy plus the optional live scan.
try:
    with open(f"{root}/deny.toml", "rb") as handle:
        deny = tomllib.load(handle)
except (OSError, tomllib.TOMLDecodeError) as err:
    fail_row("advisories", "deny.toml", f"unreadable ({err})")
    deny = None
if deny is not None:
    advisories = deny.get("advisories")
    if not isinstance(advisories, dict):
        fail_row("advisories", "deny.toml",
                 "[advisories] table missing")
    elif advisories.get("ignore", []):
        for ignored in advisories["ignore"]:
            fail_row("advisories", str(ignored),
                     "ignored advisory must be a policy exception instead")
    else:
        pass_row("advisories", "deny ignore list", "empty")
if with_advisories:
    cargo_deny = shutil.which("cargo-deny")
    if cargo_deny is None:
        fail_row("advisories", "live scan",
                 "--with-advisories requested but cargo-deny is not on PATH")
    else:
        try:
            completed = subprocess.run(
                [cargo_deny, "check", "advisories"], cwd=root,
                capture_output=True, text=True, timeout=180)
        except (OSError, subprocess.TimeoutExpired) as err:
            fail_row("advisories", "live scan", f"tool failed ({err})")
        else:
            if completed.returncode == 0:
                pass_row("advisories", "live scan",
                         "cargo deny check advisories: no findings")
            else:
                tail = (completed.stdout + completed.stderr)[-500:]
                fail_row("advisories", "live scan",
                         "cargo deny reported findings; run "
                         f"`cargo deny check advisories` for evidence: {tail!r}")
else:
    info_row("advisories", "live scan",
             "runs as the CI Cargo Deny job; --with-advisories runs it here")

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


COMMUNITY_SOURCE = "https://api.github.com/repos/graalvm/graalvm-ce-builds/releases?per_page=10"
COMMUNITY_PAGES = 3
COMMUNITY_TARGETS = {"linux-aarch64", "linux-x64", "macos-aarch64"}


def community_release(release):
    """Map one stable provider release by asset identity, never tag arithmetic."""
    if not isinstance(release, dict):
        raise ValueError("Community release must be an object")
    if release.get("draft") is True or release.get("prerelease") is True:
        return None
    if release.get("draft") is not False or release.get("prerelease") is not False:
        raise ValueError("Community release lacks stable flags")
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise ValueError("Community release lacks assets")
    cohort, targets = set(), set()
    tag = release.get("tag_name")
    for asset in assets:
        if not isinstance(asset, dict) or not isinstance(asset.get("name"), str):
            raise ValueError("Community asset lacks name")
        name = asset["name"]
        if not name.startswith("graalvm-community-jdk-25") or not name.endswith("_bin.tar.gz"):
            continue
        match = re.fullmatch(r"graalvm-community-jdk-(?:(25i[1-9]\d*)-)?"
                             r"(25(?:\.(?:0|[1-9]\d*)){2,})_"
                             r"(linux-aarch64|linux-x64|macos-aarch64|macos-x64)_bin\.tar\.gz", name)
        if not match:
            raise ValueError("malformed Community JDK25 asset")
        if match[3] not in COMMUNITY_TARGETS:
            continue
        if not isinstance(tag, str) or not re.fullmatch(r"(?:graal|jdk)-[0-9]+(?:\.[0-9]+)*", tag):
            raise ValueError("malformed Community release tag")
        official = "https://github.com/graalvm/graalvm-ce-builds/releases/"
        if release.get("html_url") != official + "tag/" + tag:
            raise ValueError("Community release provider mismatch")
        if asset.get("browser_download_url") != official + "download/" + tag + "/" + name:
            raise ValueError("Community asset URL mismatch")
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", asset.get("digest") or ""):
            raise ValueError("Community asset lacks SHA256 digest")
        if match[3] in targets:
            raise ValueError("duplicate Community target asset")
        targets.add(match[3])
        cohort.add((match[1], match[2]))
    if not targets:
        return None
    if targets != COMMUNITY_TARGETS or len(cohort) != 1:
        raise ValueError("incomplete or ambiguous Community JDK25 cohort")
    return next(iter(cohort))[1]


def community_latest(source, body):
    """At most three 10-release pages; absent/malformed cohorts fail closed."""
    if source != COMMUNITY_SOURCE and not source.startswith("file://"):
        raise ValueError("Community freshness requires official release listing")
    versions = []
    for page in range(1, COMMUNITY_PAGES + 1):
        if len(body.encode("utf-8")) > FETCH_CAP:
            raise ValueError("Community release page exceeds fetch cap")
        payload = json.loads(body)
        if not isinstance(payload, list) or len(payload) > 10:
            raise ValueError("Community freshness requires bounded release list")
        versions.extend(version for release in payload
                        if (version := community_release(release)) is not None)
        if len(payload) < 10 or source.startswith("file://") or page == COMMUNITY_PAGES:
            break
        body = fetch_text(COMMUNITY_SOURCE + f"&page={page + 1}")
    if not versions:
        raise ValueError("no maintained Community JDK25 cohort within lookup bound")
    return max(versions, key=lambda value: tuple(map(int, value.split("."))))


def sniff_latest(source, body, community_java=False):
    if community_java:
        return community_latest(source, body)
    if source == "https://www.python.org/downloads/" or source.startswith("file://"):
        match = re.search(r"Download Python (\d+\.\d+\.\d+)", body)
        if match:
            return match.group(1)
        if source == "https://www.python.org/downloads/":
            return None
    try:
        payload = json.loads(body)
    except ValueError:
        payload = None
    if payload is not None:
        if (source == "https://nodejs.org/dist/index.json" or source.startswith("file://")) \
                and isinstance(payload, list) \
                and any(isinstance(entry, dict) and "lts" in entry for entry in payload):
            versions = [entry.get("version") for entry in payload
                        if isinstance(entry, dict) and entry.get("lts")
                        and re.fullmatch(r"v24\.\d+\.\d+", entry.get("version", ""))]
            return max(versions, key=lambda v: tuple(map(int, v[1:].split(".")))) if versions else None
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
    for tool in tools + delivery_tools:
        name = tool.get("name")
        source = tool.get("source", "")
        try:
            latest = sniff_latest(source, fetch_text(source), name == "java")
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

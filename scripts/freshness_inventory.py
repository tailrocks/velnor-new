"""Load and check reviewed inventories, source pins, and policy mirrors."""

import json
import re
import sys
import tomllib

from freshness_context import parse_iso_date, parse_timestamp


CATALOG = "crates/velnor-actions-mise/src/catalog.rs"
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
    "reuse": "REUSE_VERSION",
    "python": "PYTHON_VERSION",
    "uv": "UV_VERSION",
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


def load_inventory(ctx):
    try:
        with open(ctx.inv_path, encoding="utf-8") as handle:
            inv = json.load(handle)
    except (OSError, ValueError) as err:
        print(f"check-freshness: unreadable inventory ({err})",
              file=sys.stderr)
        return False
    if not isinstance(inv, dict):
        print("check-freshness: inventory root must be an object",
              file=sys.stderr)
        return False
    ctx.inv = inv
    return check_inventory_shape(ctx)


def check_inventory_shape(ctx):
    inv = ctx.inv
    known = ("schema", "check_interval_hours", "max_exception_days",
             "checked_at", "tools", "actions", "runner", "exceptions",
             "temporary_holds")
    for key in sorted(inv):
        if key not in known:
            ctx.info_row("inventory-shape", key, "unrecognized top-level key")
    if inv.get("schema") != 1:
        ctx.fail_row("inventory-shape", "schema",
                     f"must be 1, got {inv.get('schema')!r}")
    interval = inv.get("check_interval_hours")
    ctx.interval = interval
    if not isinstance(interval, int) or interval <= 0:
        ctx.fail_row("inventory-shape", "check_interval_hours",
                     f"must be a positive int, got {interval!r}")
        ctx.interval = 24
    max_days = inv.get("max_exception_days")
    ctx.max_days = max_days
    if not isinstance(max_days, int) or max_days <= 0:
        ctx.fail_row("inventory-shape", "max_exception_days",
                     f"must be a positive int, got {max_days!r}")
        ctx.max_days = 14
    ctx.top_checked = inv.get("checked_at")
    if ctx.top_checked is not None and parse_timestamp(ctx.top_checked) is None:
        ctx.fail_row("inventory-shape", "checked_at",
                     f"malformed timestamp {ctx.top_checked!r}")
    return True


def load_policy(ctx):
    try:
        with open(ctx.path(".velnor/version-policy.toml"), "rb") as handle:
            ctx.policy = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as err:
        ctx.fail_row("policy-header", "version-policy.toml",
                     f"unreadable ({err})")


def check_policy_header(ctx):
    policy = ctx.policy
    if policy is None:
        return
    for key in sorted(policy):
        if key in ("tools", "github_runner_images", "actions",
                   "validation-tools"):
            continue
        if key not in ("schema", "channel", "registry",
                       "check_interval_hours", "max_exception_days"):
            ctx.fail_row("policy-header", key, "unknown key rejected")
    header = (("schema", 1), ("channel", "stable"),
              ("registry", policy.get("registry")),
              ("check_interval_hours", ctx.interval),
              ("max_exception_days", ctx.max_days))
    for key, want in header:
        got = policy.get(key)
        if key == "registry":
            if not isinstance(got, str) or not got.startswith("https://") \
                    or any(char.isspace() for char in got):
                ctx.fail_row("policy-header", key,
                             f"must be an https URL, got {got!r}")
            else:
                ctx.pass_row("policy-header", key, got)
        elif got != want:
            ctx.fail_row("policy-header", key, f"got={got!r} want={want!r}")
        else:
            ctx.pass_row("policy-header", key, f"{got!r}")
    if isinstance(policy.get("check_interval_hours"), int) \
            and policy["check_interval_hours"] > 24:
        ctx.fail_row("policy-header", "check_interval_hours",
                     "weakens policy: must be <= 24")
    if isinstance(policy.get("max_exception_days"), int) \
            and policy["max_exception_days"] > 14:
        ctx.fail_row("policy-header", "max_exception_days",
                     "weakens policy: must be <= 14")


def pin_row(ctx, subject, actual, expected):
    if actual is None:
        return
    if actual != expected:
        ctx.fail_row("local-pin", subject,
                     f"code={actual!r} inventory={expected!r}")
    else:
        ctx.pass_row("local-pin", subject, str(actual))


def _check_tool_pins(ctx):
    ctx.tools = ctx.inv.get("tools", [])
    seen = set()
    for tool in ctx.tools:
        if not isinstance(tool, dict):
            ctx.fail_row("inventory-shape", "(inventory tools)",
                         f"entry must be an object, got {tool!r}")
            continue
        name = tool.get("name")
        seen.add(name)
        const = EXPECTED_TOOLS.get(name)
        if const is None:
            ctx.fail_row("local-pin", f"tool {name}",
                         "inventory entry outside the expected tool set")
            continue
        pin_row(ctx, f"tool {name} ({CATALOG}::{const})",
                ctx.rust_const(CATALOG, const), tool.get("pinned"))
    for name in sorted(set(EXPECTED_TOOLS) - seen):
        ctx.fail_row("local-pin", f"tool {name}", "inventory row missing")


def _check_action_pins(ctx):
    ctx.action_pinned = {}
    for action in ctx.inv.get("actions", []):
        if not isinstance(action, dict):
            ctx.fail_row("inventory-shape", "(inventory actions)",
                         f"entry must be an object, got {action!r}")
            continue
        key = action.get("key")
        ctx.action_pinned[key] = action
        prefix = EXPECTED_ACTIONS.get(key)
        if prefix is None:
            ctx.fail_row("local-pin", f"action {key}",
                         "inventory entry outside the expected action set")
            continue
        pin_row(ctx, f"action {key} version ({ACTIONS}::{prefix}_VERSION)",
                ctx.rust_const(ACTIONS, f"{prefix}_VERSION"),
                action.get("pinned_version"))
        pin_row(ctx, f"action {key} sha ({ACTIONS}::{prefix}_SHA)",
                ctx.rust_const(ACTIONS, f"{prefix}_SHA"),
                action.get("pinned_sha"))
    for key in sorted(set(EXPECTED_ACTIONS) - set(ctx.action_pinned)):
        ctx.fail_row("local-pin", f"action {key}", "inventory row missing")


def _check_local_mirrors(ctx):
    ctx.tool_pinned = {tool.get("name"): tool.get("pinned")
                       for tool in ctx.tools}
    pin_row(ctx, "tool actionlint mirror (capabilities.rs)",
            ctx.rust_const(CAPABILITIES, "ACTIONLINT_VERSION"),
            ctx.tool_pinned.get("actionlint"))
    pin_row(ctx, "tool shellcheck mirror (tools.rs)",
            ctx.rust_const(TOOLS, "SHELLCHECK_VERSION"),
            ctx.tool_pinned.get("shellcheck"))
    pin_row(ctx, "action asamarts/alint binary mirror (render.rs)",
            ctx.rust_const(RENDERER, "ALINT_BINARY_VERSION"),
            ctx.action_pinned.get("asamarts/alint", {}).get("pinned_version"))


def _check_runner_pins(ctx):
    ctx.runner = ctx.inv.get("runner") or {}
    pin_row(ctx, "runner default (config.rs::RUNNER_LABEL_BRIDGE)",
            ctx.rust_const(CONFIG, "RUNNER_LABEL_BRIDGE"),
            ctx.runner.get("default"))
    ctx.supported = ctx.runner.get("supported", [])
    if not isinstance(ctx.supported, list) or not ctx.supported:
        ctx.fail_row("local-pin", "runner supported",
                     "must be a non-empty label list")
        return
    for label in ctx.supported:
        if not isinstance(label, str) or "latest" in label:
            ctx.fail_row("local-pin", "runner supported",
                         f"unversioned label rejected: {label!r}")
    if ctx.runner.get("default") not in ctx.supported:
        ctx.fail_row("local-pin", "runner supported",
                     f"default {ctx.runner.get('default')!r} not listed")
    else:
        ctx.pass_row("local-pin", "runner supported",
                     ",".join(ctx.supported))


def check_local_pins(ctx):
    _check_tool_pins(ctx)
    _check_action_pins(ctx)
    _check_local_mirrors(ctx)
    _check_runner_pins(ctx)


def _check_policy_tools_and_runner(ctx):
    policy_tools = ctx.policy.get("tools", {})
    for name in sorted(set(EXPECTED_TOOLS) | set(policy_tools)):
        want = ctx.tool_pinned.get(name)
        got = policy_tools.get(name)
        if name not in EXPECTED_TOOLS:
            ctx.fail_row("policy-mirror", f"tool {name}",
                         "policy entry outside the expected tool set")
        elif got != want:
            ctx.fail_row("policy-mirror", f"tool {name}",
                         f"policy={got!r} inventory={want!r}")
        else:
            ctx.pass_row("policy-mirror", f"tool {name}", str(got))
    images = (ctx.policy.get("github_runner_images") or {}).get("linux_x64") or {}
    if images.get("default") != ctx.runner.get("default"):
        ctx.fail_row("policy-mirror", "runner default",
                     f"policy={images.get('default')!r} "
                     f"inventory={ctx.runner.get('default')!r}")
    else:
        ctx.pass_row("policy-mirror", "runner default",
                     str(images.get("default")))
    if sorted(images.get("supported", [])) != sorted(ctx.supported):
        ctx.fail_row("policy-mirror", "runner supported",
                     f"policy={images.get('supported')!r} "
                     f"inventory={ctx.supported!r}")
    else:
        ctx.pass_row("policy-mirror", "runner supported",
                     ",".join(images.get("supported", [])))


def _policy_action_rows(ctx):
    policy_actions = {}
    for entry in ctx.policy.get("actions", []):
        if not isinstance(entry, dict):
            ctx.fail_row("policy-mirror", "actions",
                         f"entry must be a table, got {entry!r}")
            continue
        _check_policy_action_shape(ctx, entry)
        name = entry.get("name")
        if name in policy_actions:
            ctx.fail_row("policy-mirror", f"action {name}", "duplicate entry")
        policy_actions[name] = entry
    _compare_policy_actions(ctx, policy_actions)


def _check_policy_action_shape(ctx, entry):
    for key in sorted(entry):
        if key not in ("name", "version", "sha", "reviewed"):
            ctx.fail_row("policy-mirror", f"action {entry.get('name')}",
                         f"unknown key rejected: {key}")
    name = entry.get("name")
    if parse_iso_date(entry.get("reviewed", "")) is None:
        ctx.fail_row("policy-mirror", f"action {name}",
                     f"reviewed must be YYYY-MM-DD, "
                     f"got {entry.get('reviewed')!r}")


def _compare_policy_actions(ctx, policy_actions):
    for key in sorted(set(ctx.action_pinned) | set(policy_actions)):
        if key not in ctx.action_pinned:
            ctx.fail_row("policy-mirror", f"action {key}",
                         "policy entry without an inventory row")
            continue
        if key not in policy_actions:
            ctx.fail_row("policy-mirror", f"action {key}",
                         "inventory row without a policy entry")
            continue
        want = ctx.action_pinned[key]
        got = policy_actions[key]
        _compare_policy_action(ctx, key, want, got)


def _compare_policy_action(ctx, key, want, got):
    if got.get("version") != want.get("pinned_version") or \
            got.get("sha") != want.get("pinned_sha"):
        ctx.fail_row("policy-mirror", f"action {key}",
                     f"policy={got.get('version')!r}@{got.get('sha')!r} "
                     f"inventory={want.get('pinned_version')!r}@"
                     f"{want.get('pinned_sha')!r}")
    elif not re.fullmatch(r"[0-9a-f]{40}", got.get("sha") or ""):
        ctx.fail_row("policy-mirror", f"action {key}",
                     f"sha must be 40 hex, got {got.get('sha')!r}")
    else:
        ctx.pass_row("policy-mirror", f"action {key}",
                     f"{got.get('version')}@{got.get('sha')}")


def check_policy_mirror(ctx):
    if ctx.policy is None:
        return
    _check_policy_tools_and_runner(ctx)
    _policy_action_rows(ctx)

import glob as globmod
import os
import re

from report import fail_row, pass_row, parse_iso_date
from rust_pin_parser import SOURCE_CAP, extract_rust_const


CATALOG = "crates/velnor-actions-mise/src/catalog.rs"
ACTIONS = "crates/velnor-actions-actionlint/src/actions.rs"
TOOLS = "crates/velnor-actions-actionlint/src/tools.rs"
CAPABILITIES = "crates/velnor-actions-actionlint/src/capabilities.rs"
CONFIG = "crates/velnor-actions-actionlint/src/config.rs"
RENDERER = "crates/velnor-actions-workflow-renderer/src/render.rs"
MUTANTS = ".cargo/mutants.toml"
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


def rust_const(root, path, name):
    try:
        with open(f"{root}/{path}", "rb") as handle:
            source = handle.read(SOURCE_CAP + 1)
    except OSError as err:
        fail_row("local-pin", f"{path}::{name}", f"unreadable ({err})")
        return None
    try:
        return extract_rust_const(source, name)
    except ValueError as err:
        fail_row("local-pin", f"{path}::{name}", str(err))
        return None


def pin_row(subject, actual, expected):
    if actual is None:
        return
    if actual != expected:
        fail_row("local-pin", subject,
                 f"code={actual!r} inventory={expected!r}")
    else:
        pass_row("local-pin", subject, str(actual))


def check_tool_pins(root, tools):
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
                rust_const(root, CATALOG, const), tool.get("pinned"))
    for name in sorted(set(EXPECTED_TOOLS) - seen_tools):
        fail_row("local-pin", f"tool {name}", "inventory row missing")


def check_action_pins(root, actions):
    action_pinned = {}
    for action in actions:
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
                rust_const(root, ACTIONS, f"{prefix}_VERSION"),
                action.get("pinned_version"))
        pin_row(f"action {key} sha ({ACTIONS}::{prefix}_SHA)",
                rust_const(root, ACTIONS, f"{prefix}_SHA"),
                action.get("pinned_sha"))
    for key in sorted(set(EXPECTED_ACTIONS) - set(action_pinned)):
        fail_row("local-pin", f"action {key}", "inventory row missing")
    return action_pinned


def check_mirror_pins(root, tools, action_pinned, runner):
    tool_pinned = {tool.get("name"): tool.get("pinned") for tool in tools}
    pin_row("tool actionlint mirror (capabilities.rs)",
            rust_const(root, CAPABILITIES, "ACTIONLINT_VERSION"),
            tool_pinned.get("actionlint"))
    pin_row("tool shellcheck mirror (tools.rs)",
            rust_const(root, TOOLS, "SHELLCHECK_VERSION"),
            tool_pinned.get("shellcheck"))
    pin_row("action asamarts/alint binary mirror (render.rs)",
            rust_const(root, RENDERER, "ALINT_BINARY_VERSION"),
            action_pinned.get("asamarts/alint", {}).get("pinned_version"))
    pin_row("runner default (config.rs::RUNNER_LABEL_BRIDGE)",
            rust_const(root, CONFIG, "RUNNER_LABEL_BRIDGE"),
            runner.get("default"))
    supported = runner.get("supported", [])
    if not isinstance(supported, list) or not supported:
        fail_row("local-pin", "runner supported",
                 "must be a non-empty label list")
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
    return tool_pinned, supported


def check_policy_mirror(policy, tool_pinned, action_pinned, runner, supported):
    if policy is None:
        return
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
        pass_row("policy-mirror", "runner default", str(images.get("default")))
    if sorted(images.get("supported", [])) != sorted(supported):
        fail_row("policy-mirror", "runner supported",
                 f"policy={images.get('supported')!r} "
                 f"inventory={supported!r}")
    else:
        pass_row("policy-mirror", "runner supported",
                 ",".join(images.get("supported", [])))
    check_policy_actions(policy, action_pinned)


def check_policy_actions(policy, action_pinned):
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
    compare_policy_actions(action_pinned, policy_actions)


def compare_policy_actions(action_pinned, policy_actions):
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


def check_mutants(root, policy):
    try:
        with open(f"{root}/{MUTANTS}", encoding="utf-8") as handle:
            mutants_text = handle.read()
    except OSError as err:
        fail_row("local-pin", MUTANTS, f"unreadable ({err})")
        mutants_text = None
    if mutants_text is None or policy is None:
        return
    pinned_tools = policy.get("validation-tools", {})
    if "cargo-mutants" not in pinned_tools:
        fail_row("local-pin", "validation-tools/cargo-mutants",
                 "policy pin missing")
    else:
        want = pinned_tools["cargo-mutants"]
        match = re.search(r'^# pinned: cargo-mutants = "([^"]+)"',
                          mutants_text, re.MULTILINE)
        if not match:
            tick = chr(96)
            fail_row("local-pin", "validation-tools/cargo-mutants",
                     f"{MUTANTS} lacks a {tick}# pinned: cargo-mutants = \"x\"{tick} line")
        elif match.group(1) != want:
            fail_row("local-pin", "validation-tools/cargo-mutants",
                     f"mutants pin={match.group(1)!r} policy={want!r}")
        else:
            pass_row("local-pin", "validation-tools/cargo-mutants", want)
    check_mutant_scope(root, mutants_text)


def check_mutant_scope(root, mutants_text):
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


def run_local_pins(root, inv, policy):
    tools = inv.get("tools", [])
    check_tool_pins(root, tools)
    action_pinned = check_action_pins(root, inv.get("actions", []))
    runner = inv.get("runner") or {}
    tool_pinned, supported = check_mirror_pins(
        root, tools, action_pinned, runner)
    check_policy_mirror(policy, tool_pinned, action_pinned, runner, supported)
    check_mutants(root, policy)
    return tools, action_pinned, runner, supported

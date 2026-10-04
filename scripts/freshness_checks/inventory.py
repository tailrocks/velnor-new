import json
import sys
import tomllib

from report import fail_row, info_row, pass_row, parse_timestamp


def load_inventory(inv_path):
    try:
        with open(inv_path, encoding="utf-8") as handle:
            inv = json.load(handle)
    except (OSError, ValueError) as err:
        print(f"check-freshness: unreadable inventory ({err})", file=sys.stderr)
        raise SystemExit(1)
    if not isinstance(inv, dict):
        print("check-freshness: inventory root must be an object", file=sys.stderr)
        raise SystemExit(1)
    return inv


def validate_inventory(inv):
    """Validate inventory shape and return the checked cadence and date."""
    known = ("schema", "check_interval_hours", "max_exception_days",
             "checked_at", "tools", "actions", "runner", "exceptions",
             "temporary_holds")
    for key in sorted(inv):
        if key not in known:
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
    return interval, max_days, top_checked


def load_policy(root, interval, max_days):
    try:
        with open(f"{root}/.velnor/version-policy.toml", "rb") as handle:
            policy = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as err:
        fail_row("policy-header", "version-policy.toml", f"unreadable ({err})")
        return None
    check_policy_header(policy, interval, max_days)
    return policy


def check_policy_header(policy, interval, max_days):
    for key in sorted(policy):
        if key in ("tools", "github_runner_images", "actions", "validation-tools"):
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

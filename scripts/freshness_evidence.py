"""Check recorded freshness evidence, exceptions, and deny policy."""

import os
import shutil
import subprocess
import tomllib

from freshness_context import (
    norm_version,
    parse_iso_date,
    parse_timestamp,
)
from freshness_inventory import EXPECTED_ACTIONS, EXPECTED_TOOLS


BLESSED_STANDING = "asamarts/alint"


def evidence_age_hours(ctx, entry):
    stamp = entry.get("checked_at", ctx.top_checked)
    moment = parse_timestamp(stamp)
    if moment is None:
        return (None, stamp)
    return ((ctx.now - moment).total_seconds() / 3600, stamp)


def freshness_row(ctx, subject, entry, pinned, qualified, latest=None,
                  pin_for_latest=None):
    source = entry.get("source", "")
    if not isinstance(source, str) or "://" not in source:
        ctx.fail_row("upstream-freshness", subject, "missing source URL")
        return
    if not source.startswith(("https://", "http://", "file://")):
        ctx.fail_row("upstream-freshness", subject,
                     f"unsupported source scheme: {source!r}")
        return
    age, stamp = evidence_age_hours(ctx, entry)
    if age is None:
        ctx.fail_row("upstream-freshness", subject,
                     f"missing or malformed check timestamp {stamp!r}")
        return
    if future_evidence(ctx, stamp):
        ctx.fail_row("upstream-freshness", subject,
                     f"check timestamp {stamp} is in the future")
        return
    if _held_row(ctx, subject, entry, stamp):
        return
    if entry.get("status") != "current":
        ctx.fail_row("upstream-freshness", subject,
                     f"status={entry.get('status')!r}: refresh required, "
                     f"never current (source {source}, checked {stamp})")
        return
    if age > ctx.interval:
        ctx.fail_row("upstream-freshness", subject,
                     f"stale evidence: checked {stamp} ({age:.1f}h ago, "
                     f"interval {ctx.interval}h)")
        return
    if qualified != pinned:
        ctx.fail_row("upstream-freshness", subject,
                     f"unqualified pin: pinned={pinned!r} "
                     f"qualified={qualified!r}")
        return
    _check_latest(ctx, subject, source, stamp, pinned, latest, pin_for_latest)


def future_evidence(ctx, stamp):
    """Reject future date-only evidence by UTC day and timestamps by skew."""
    text = stamp.strip() if isinstance(stamp, str) else ""
    moment = parse_timestamp(text)
    if moment is None:
        return False
    date_only = len(text) == 10 and text[4:5] == "-" and text[7:8] == "-" \
        and "T" not in text and ":" not in text
    if date_only:
        return moment.date() > ctx.now.date()
    return (ctx.now - moment).total_seconds() / 3600 < -0.1


def _held_row(ctx, subject, entry, stamp):
    if entry.get("status") != "held":
        return False
    if subject not in ctx.hold_keys and entry.get("key", subject) not in ctx.hold_keys:
        ctx.fail_row("upstream-freshness", subject,
                     "status=held without a covering temporary hold")
    else:
        ctx.pass_row("upstream-freshness", subject, f"held, evidence {stamp}")
    return True


def _check_latest(ctx, subject, source, stamp, pinned, latest, pin_for_latest):
    compared_pin = (pin_for_latest if pin_for_latest is not None
                    else (pinned if isinstance(pinned, str) else ""))
    if latest is not None and norm_version(latest) != norm_version(compared_pin):
        ctx.fail_row("upstream-freshness", subject,
                     f"stale pin: pinned={pinned!r} latest={latest!r} "
                     f"(source {source}, checked {stamp})")
        return
    ctx.pass_row("upstream-freshness", subject,
                 f"current, evidence {stamp}")


def check_recorded_freshness(ctx):
    ctx.holds = ctx.inv.get("temporary_holds", [])
    ctx.hold_keys = {hold.get("key") for hold in ctx.holds
                     if isinstance(hold, dict)}
    for tool in ctx.tools:
        freshness_row(ctx, tool.get("name"), tool, tool.get("pinned"),
                      tool.get("qualified"), tool.get("latest"))
    for key, action in sorted(ctx.action_pinned.items()):
        pinned = (action.get("pinned_version"), action.get("pinned_sha"))
        qualified = (action.get("qualified_version"),
                     action.get("qualified_sha"))
        freshness_row(ctx, key, action, pinned, qualified,
                      action.get("latest"), action.get("pinned_version"))
    freshness_row(ctx, "runner", ctx.runner, ctx.runner.get("default"),
                  ctx.runner.get("default"))


def check_dated(ctx, entry, check, today, known_subjects):
    if not isinstance(entry, dict):
        ctx.fail_row(check, "(inventory exceptions)",
                     f"entry must be an object, got {entry!r}")
        return
    subject = entry.get("key", "<unnamed hold>")
    required = ("held_version", "owner", "issue", "reason", "granted", "expires")
    missing = [key for key in required if not entry.get(key)]
    if missing:
        ctx.fail_row(check, subject, f"missing {','.join(missing)}")
        return
    granted = parse_iso_date(entry["granted"])
    expires = parse_iso_date(entry["expires"])
    if granted is None or expires is None:
        ctx.fail_row(check, subject, "granted/expires must be YYYY-MM-DD")
    elif granted > today:
        ctx.fail_row(check, subject, f"granted {granted} is in the future")
    elif expires <= granted:
        ctx.fail_row(check, subject,
                     f"inverted window: expires {expires} <= granted {granted}")
    elif (expires - granted).days > ctx.max_days:
        ctx.fail_row(check, subject,
                     f"span {(expires - granted).days}d exceeds max "
                     f"{ctx.max_days}d")
    elif expires < today:
        ctx.fail_row(check, subject,
                     f"expired {expires} (renewal needs new review + evidence)")
    elif subject not in known_subjects:
        ctx.fail_row(check, subject,
                     "hold subject matches no inventoried tool, action, "
                     "runner label, or locked package")
    else:
        ctx.pass_row(check, subject, f"expires {expires}")


def _known_subjects(ctx):
    lock_names = {entry.get("name") for entry in (ctx.locked or [])}
    subjects = (set(EXPECTED_TOOLS) | set(EXPECTED_ACTIONS) | lock_names
                | set(ctx.supported))
    if ctx.runner.get("default"):
        subjects.add(ctx.runner.get("default"))
    return subjects


def _check_temporary_holds(ctx, today, known_subjects):
    for hold in ctx.holds:
        check_dated(ctx, hold, "exception-expiry", today, known_subjects)
    if not ctx.holds:
        ctx.pass_row("exception-expiry", "(none)", "no temporary holds")


def _check_standing_record(ctx, entry, reviewed_alint):
    if not isinstance(entry, dict):
        ctx.fail_row("standing-exception", "(inventory exceptions)",
                     f"entry must be an object, got {entry!r}")
        return
    subject = entry.get("key", "<unnamed hold>")
    if entry.get("expires") is not None:
        check_dated(ctx, entry, "standing-exception", ctx.now.date(),
                    _known_subjects(ctx))
    elif subject != BLESSED_STANDING:
        ctx.fail_row("standing-exception", subject,
                     "standing hold without a spec blessing "
                     "(only asamarts/alint is blessed)")
    else:
        _check_blessed_standing(ctx, subject, entry, reviewed_alint)


def _check_blessed_standing(ctx, subject, entry, reviewed_alint):
    required = ("kind", "expiry_policy", "blessed_by", "tag")
    missing = [key for key in required if not entry.get(key)]
    if missing:
        ctx.fail_row("standing-exception", subject,
                     f"blessed standing exception lacks {','.join(missing)}")
    elif entry.get("tag") != reviewed_alint:
        ctx.fail_row("standing-exception", subject,
                     f"blessed tag {entry.get('tag')!r} != reviewed pin "
                     f"{reviewed_alint!r}: re-bless on pin moves")
    else:
        ctx.pass_row("standing-exception", subject,
                     f"blessed mutable tag {entry.get('tag')}")


def check_exceptions(ctx):
    today = ctx.now.date()
    known_subjects = _known_subjects(ctx)
    _check_temporary_holds(ctx, today, known_subjects)
    reviewed_alint = (ctx.action_pinned.get(BLESSED_STANDING) or {}).get(
        "pinned_version")
    for entry in ctx.inv.get("exceptions", []):
        _check_standing_record(ctx, entry, reviewed_alint)


def _workspace_label(relative):
    return relative or "."


def _check_deny_policy(ctx, relative):
    workspace_dir = os.path.join(relative, "deny.toml") if relative \
        else "deny.toml"
    try:
        with open(ctx.path(workspace_dir), "rb") as handle:
            deny = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as err:
        ctx.fail_row("advisories", workspace_dir, f"unreadable ({err})")
        return
    advisories = deny.get("advisories")
    if not isinstance(advisories, dict):
        ctx.fail_row("advisories", workspace_dir,
                     "[advisories] table missing")
        return
    ignored = advisories.get("ignore")
    if not isinstance(ignored, list):
        ctx.fail_row("advisories", workspace_dir,
                     "[advisories].ignore must be an empty array")
    elif ignored:
        for advisory in ignored:
            ctx.fail_row("advisories", f"{workspace_dir}:{advisory}",
                         "ignored advisory must be a policy exception instead")
    else:
        ctx.pass_row("advisories", f"{workspace_dir} ignore list", "empty")


def _check_live_advisories(ctx, relative):
    cargo_deny = shutil.which("cargo-deny")
    if cargo_deny is None:
        ctx.fail_row("advisories", "live scan",
                     "--with-advisories requested but cargo-deny is not on PATH")
        return
    workspace_dir = os.path.join(ctx.root, relative)
    manifest = os.path.join(workspace_dir, "Cargo.toml")
    deny_config = os.path.join(workspace_dir, "deny.toml")
    subject = os.path.relpath(manifest, ctx.root)
    try:
        completed = subprocess.run(
            [cargo_deny, "--locked", "--config", deny_config,
             "--manifest-path", manifest, "check", "advisories"], cwd=ctx.root,
            capture_output=True, text=True, timeout=180)
    except (OSError, subprocess.TimeoutExpired) as err:
        ctx.fail_row("advisories", subject, f"live scan failed ({err})")
    else:
        if completed.returncode == 0:
            ctx.pass_row("advisories", subject,
                         "cargo deny check advisories: no findings")
        else:
            tail = (completed.stdout + completed.stderr)[-500:]
            ctx.fail_row("advisories", subject,
                         "cargo deny reported findings; run "
                         f"`cargo deny check advisories` for evidence: {tail!r}")


def check_advisories(ctx):
    workspaces = ctx.workspace_roots
    if not workspaces:
        ctx.fail_row("advisories", "(workspaces)",
                     "no Cargo workspaces discovered")
        return
    for relative, _ in workspaces:
        _check_deny_policy(ctx, relative)
    if ctx.with_advisories:
        for relative, _ in workspaces:
            _check_live_advisories(ctx, relative)
    else:
        ctx.info_row("advisories", "live scan",
                     "runs as the CI Cargo Deny job; "
                     "--with-advisories runs it here")

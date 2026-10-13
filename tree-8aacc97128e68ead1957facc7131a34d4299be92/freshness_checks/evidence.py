from pins import EXPECTED_ACTIONS, EXPECTED_TOOLS
from report import fail_row, pass_row, parse_iso_date, parse_timestamp, norm_version


def temporary_hold_keys(holds):
    return {hold.get("key") for hold in holds if isinstance(hold, dict)}


def evidence_age_hours(entry, top_checked, now):
    stamp = entry.get("checked_at", top_checked)
    moment = parse_timestamp(stamp)
    if moment is None:
        return (None, stamp)
    return ((now - moment).total_seconds() / 3600, stamp)


def freshness_row(subject, entry, pinned, qualified, interval, top_checked,
                  now, hold_keys, latest=None, pin_for_latest=None):
    source = entry.get("source", "")
    if not isinstance(source, str) or "://" not in source:
        fail_row("upstream-freshness", subject, "missing source URL")
        return
    if not source.startswith(("https://", "http://", "file://")):
        fail_row("upstream-freshness", subject,
                 f"unsupported source scheme: {source!r}")
        return
    age, stamp = evidence_age_hours(entry, top_checked, now)
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
    compare_to = pin_for_latest if pin_for_latest is not None else \
        (pinned if isinstance(pinned, str) else "")
    if latest is not None and norm_version(latest) != norm_version(compare_to):
        fail_row("upstream-freshness", subject,
                 f"stale pin: pinned={pinned!r} latest={latest!r} "
                 f"(source {source}, checked {stamp})")
        return
    pass_row("upstream-freshness", subject,
             f"current, evidence {stamp}")


def run_upstream_freshness(tools, action_pinned, runner, interval,
                           top_checked, now, holds):
    hold_keys = temporary_hold_keys(holds)
    for tool in tools:
        name = tool.get("name")
        freshness_row(name, tool, tool.get("pinned"), tool.get("qualified"),
                      interval, top_checked, now, hold_keys, tool.get("latest"))
    for key, action in sorted(action_pinned.items()):
        pinned = (action.get("pinned_version"), action.get("pinned_sha"))
        qualified = (action.get("qualified_version"),
                     action.get("qualified_sha"))
        freshness_row(key, action, pinned, qualified, interval, top_checked,
                      now, hold_keys, action.get("latest"),
                      action.get("pinned_version"))
    freshness_row("runner", runner, runner.get("default"),
                  runner.get("default"), interval, top_checked, now, hold_keys)


def check_dated(entry, check, today, max_days, known_subjects):
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


def run_exceptions(inv, action_pinned, locked, supported, runner,
                   max_days, now):
    today = now.date()
    lock_names = {entry.get("name") for entry in (locked or [])}
    known_subjects = set(EXPECTED_TOOLS) | set(EXPECTED_ACTIONS) | \
        lock_names | set(supported)
    if runner.get("default"):
        known_subjects.add(runner.get("default"))
    holds = inv.get("temporary_holds", [])
    for hold in holds:
        check_dated(hold, "exception-expiry", today, max_days, known_subjects)
    if not holds:
        pass_row("exception-expiry", "(none)", "no temporary holds")
    check_standing_exceptions(inv, action_pinned, today, max_days,
                              known_subjects)


def check_standing_exceptions(inv, action_pinned, today, max_days,
                              known_subjects):
    blessed_standing = "asamarts/alint"
    reviewed_alint = (action_pinned.get(blessed_standing) or {}).get(
        "pinned_version")
    for exc in inv.get("exceptions", []):
        if not isinstance(exc, dict):
            fail_row("standing-exception", "(inventory exceptions)",
                     f"entry must be an object, got {exc!r}")
            continue
        subject = exc.get("key", "<unnamed hold>")
        if exc.get("expires") is None:
            check_blessed_exception(exc, subject, blessed_standing,
                                    reviewed_alint)
        else:
            check_dated(exc, "standing-exception", today, max_days,
                        known_subjects)


def check_blessed_exception(exc, subject, blessed_standing, reviewed_alint):
    if subject != blessed_standing:
        fail_row("standing-exception", subject,
                 "standing hold without a spec blessing "
                 "(only asamarts/alint is blessed)")
        return
    missing = [key for key in ("kind", "expiry_policy", "blessed_by", "tag")
               if not exc.get(key)]
    if missing:
        fail_row("standing-exception", subject,
                 f"blessed standing exception lacks {','.join(missing)}")
    elif exc.get("tag") != reviewed_alint:
        fail_row("standing-exception", subject,
                 f"blessed tag {exc.get('tag')!r} != reviewed pin "
                 f"{reviewed_alint!r}: re-bless on pin moves")
    else:
        pass_row("standing-exception", subject,
                 f"blessed mutable tag {exc.get('tag')}")

import datetime
import json
import re
import sys


failures = []


def row(check, subject, status, detail=""):
    """Machine-readable inventory row: row: {compact JSON} on stdout."""
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


def parse_iso_date(text):
    """Strict YYYY-MM-DD date or None."""
    if not isinstance(text, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", text):
        return None
    try:
        return datetime.date.fromisoformat(text)
    except ValueError:
        return None


def norm_version(text):
    """Compare versions ignoring a leading v and build metadata."""
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


def finish():
    if failures:
        print("check-freshness: FAIL", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("check-freshness: PASS")
    return 0

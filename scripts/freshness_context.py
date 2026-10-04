"""Shared state and row formatting for the repository freshness gate."""

import datetime
import json
import re
import sys
from dataclasses import dataclass, field


def parse_iso_date(text):
    """Return a strict YYYY-MM-DD date, or None."""
    if not isinstance(text, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", text):
        return None
    try:
        return datetime.date.fromisoformat(text)
    except ValueError:
        return None


def norm_version(text):
    """Compare versions without a leading v or build metadata."""
    return (text or "").strip().removeprefix("v").split("+", 1)[0]


def parse_timestamp(text):
    """Parse a date or timestamp as an aware UTC datetime, or None."""
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


@dataclass
class FreshnessContext:
    """State shared by the gate's explicit repository policy checks."""

    root: str
    inv_path: str
    check_upstream: bool
    with_advisories: bool
    failures: list = field(default_factory=list)
    now: datetime.datetime = field(
        default_factory=lambda: datetime.datetime.now(datetime.timezone.utc))
    inv: dict = field(default_factory=dict)
    interval: int = 24
    max_days: int = 14
    top_checked: object = None
    policy: object = None
    tools: list = field(default_factory=list)
    action_pinned: dict = field(default_factory=dict)
    tool_pinned: dict = field(default_factory=dict)
    runner: dict = field(default_factory=dict)
    supported: list = field(default_factory=list)
    locked: object = None
    member_names: set = field(default_factory=set)
    holds: list = field(default_factory=list)
    hold_keys: set = field(default_factory=set)

    def path(self, relative):
        return f"{self.root}/{relative}"

    def row(self, check, subject, status, detail=""):
        payload = json.dumps(
            {"check": check, "subject": subject,
             "status": status, "detail": detail},
            separators=(",", ":"), sort_keys=True)
        print(f"row: {payload}")

    def fail_row(self, check, subject, detail):
        self.row(check, subject, "fail", detail)
        self.failures.append(f"{check} {subject}: {detail}")

    def pass_row(self, check, subject, detail=""):
        self.row(check, subject, "pass", detail)
        print(f"ok: {check} {subject} {detail}".rstrip())

    def info_row(self, check, subject, detail=""):
        self.row(check, subject, "info", detail)

    def rust_const(self, path, name):
        """Extract a public string constant, emitting a row on failure."""
        try:
            with open(self.path(path), encoding="utf-8") as handle:
                text = handle.read()
        except OSError as err:
            self.fail_row("local-pin", f"{path}::{name}",
                          f"unreadable ({err})")
            return None
        match = re.search(rf'pub const {name}:\s*&str\s*=\s*"([^"]*)";', text)
        if not match:
            self.fail_row("local-pin", f"{path}::{name}",
                          f"missing const {name}")
            return None
        return match.group(1)

    def finish(self):
        if self.failures:
            print("check-freshness: FAIL", file=sys.stderr)
            for failure in self.failures:
                print(f"  - {failure}", file=sys.stderr)
            return 1
        print("check-freshness: PASS")
        return 0

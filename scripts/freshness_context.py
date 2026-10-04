"""Shared state and row formatting for the repository freshness gate."""

import datetime
import json
import re
import sys
from dataclasses import dataclass, field

RUST_SOURCE_CAP = 256 * 1024
_RAW_STRING_START = re.compile(r'(?:br|cr|r)(#+)?"')
_OPEN_TO_CLOSE = {"(": ")", "[": "]", "{": "}"}
_CLOSE_DELIMITERS = frozenset(_OPEN_TO_CLOSE.values())
_SIMPLE_ESCAPES = frozenset("nrt0\\\"'")


def _skip_token_group(tokens, index):
    opening = tokens[index][1]
    if opening not in _OPEN_TO_CLOSE:
        raise ValueError("invalid macro token tree")
    stack = [_OPEN_TO_CLOSE[opening]]
    for cursor in range(index + 1, len(tokens)):
        kind, value = tokens[cursor]
        if kind != "punct":
            continue
        if value in _OPEN_TO_CLOSE:
            stack.append(_OPEN_TO_CLOSE[value])
        elif value in _CLOSE_DELIMITERS:
            if value != stack[-1]:
                raise ValueError("mismatched macro token tree")
            stack.pop()
            if not stack:
                return cursor + 1
    raise ValueError("unterminated macro token tree")


def _without_macro_bodies(tokens):
    filtered = []
    index = 0
    while index < len(tokens):
        macro_rules = [("ident", "macro_rules"), ("punct", "!")]
        if tokens[index:index + 2] == macro_rules \
                and index + 3 < len(tokens) \
                and tokens[index + 2][0] == "ident" \
                and tokens[index + 3][0] == "punct" \
                and tokens[index + 3][1] in _OPEN_TO_CLOSE:
            index = _skip_token_group(tokens, index + 3)
            continue
        if tokens[index] == ("punct", "!") \
                and (index == 0 or tokens[index - 1] != ("punct", "#")) \
                and index + 1 < len(tokens) \
                and tokens[index + 1][0] == "punct" \
                and tokens[index + 1][1] in _OPEN_TO_CLOSE:
            index = _skip_token_group(tokens, index + 1)
            continue
        filtered.append(tokens[index])
        index += 1
    return filtered


def _top_level_const_matches(tokens, name):
    matches = []
    stack = []
    declaration = [("ident", "const"), ("ident", name)]
    for index, (kind, value) in enumerate(tokens):
        if kind == "punct" and value in _OPEN_TO_CLOSE:
            stack.append(_OPEN_TO_CLOSE[value])
            continue
        if kind == "punct" and value in _CLOSE_DELIMITERS:
            if not stack or stack.pop() != value:
                raise ValueError("mismatched Rust source delimiter")
            continue
        if not stack and tokens[index:index + 2] == declaration:
            matches.append(index)
    if stack:
        raise ValueError("unterminated Rust source delimiter")
    return matches


def _has_conditional_crate_attribute(tokens):
    index = 0
    while index < len(tokens):
        if tokens[index:index + 3] == [
                ("punct", "#"), ("punct", "!"), ("punct", "[")]:
            if index + 3 < len(tokens) \
                    and tokens[index + 3] in (
                        ("ident", "cfg"), ("ident", "cfg_attr")):
                return True
            index = _skip_token_group(tokens, index + 2)
            continue
        if tokens[index][0] == "punct" \
                and tokens[index][1] in _OPEN_TO_CLOSE:
            index = _skip_token_group(tokens, index)
            continue
        index += 1
    return False


def _rust_tokens(source):
    """Tokenize enough Rust to identify literal pin authorities safely."""
    tokens = []
    index = 0
    while index < len(source):
        char = source[index]
        if char.isspace():
            index += 1
            continue
        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            index = len(source) if end < 0 else end + 1
            continue
        if source.startswith("/*", index):
            index = _skip_block_comment(source, index)
            continue
        raw_start = _RAW_STRING_START.match(source, index)
        if raw_start is not None:
            hashes = raw_start.group(1) or ""
            content_start = raw_start.end()
            terminator = '"' + hashes
            end = source.find(terminator, content_start)
            if end < 0:
                raise ValueError("unterminated raw string")
            tokens.append(("literal", None))
            index = end + len(terminator)
            continue
        if source.startswith(('b"', 'c"'), index):
            family = "byte" if source[index] == "b" else "c"
            index = _read_string(source, index + 1, family)[0]
            tokens.append(("literal", None))
            continue
        if char == '"':
            end, value = _read_string(source, index)
            tokens.append(("string", value))
            index = end
            continue
        if source.startswith("b'", index):
            index = _skip_char(source, index + 1, byte=True)
            tokens.append(("literal", None))
            continue
        if char == "'":
            char_end = _char_end(source, index)
            if char_end is not None:
                tokens.append(("literal", None))
                index = char_end
                continue
        if char == "_" or char.isalpha():
            raw_identifier = source.startswith("r#", index) \
                    and index + 2 < len(source) \
                    and source[index + 2].isidentifier()
            start = index + 2 if raw_identifier else index
            index = start + 1
            while index < len(source) and (source[index] == "_"
                                            or source[index].isalnum()):
                index += 1
            tokens.append(("ident", source[start:index]))
            continue
        tokens.append(("punct", char))
        index += 1
    return _without_macro_bodies(tokens)


def _skip_block_comment(source, index):
    depth = 1
    index += 2
    while index < len(source) and depth:
        if source.startswith("/*", index):
            depth += 1
            index += 2
        elif source.startswith("*/", index):
            depth -= 1
            index += 2
        else:
            index += 1
    if depth:
        raise ValueError("unterminated block comment")
    return index


def _read_string(source, index, family="str"):
    start = index + 1
    index = start
    escaped = False
    while index < len(source):
        if source[index] == "\\":
            index = _skip_escape(source, index, family=family, continuation=True)
            escaped = True
        elif source[index] == '"':
            value = None if escaped else source[start:index]
            return index + 1, value
        else:
            index += 1
    raise ValueError("unterminated string")


def _skip_escape(source, index, *, family="str", continuation=False):
    cursor = index + 1
    if cursor >= len(source):
        raise ValueError("unterminated escape")
    escape = source[cursor]
    if escape in _SIMPLE_ESCAPES and not (family == "c" and escape == "0"):
        return cursor + 1
    if escape == "x":
        digits = source[cursor + 1:cursor + 3]
        if len(digits) != 2 or any(char not in "0123456789abcdefABCDEF"
                                   for char in digits):
            raise ValueError("invalid hexadecimal escape")
        value = int(digits, 16)
        if (family == "str" and value > 0x7f) \
                or (family == "c" and value == 0):
            raise ValueError("invalid hexadecimal escape for literal family")
        return cursor + 3
    if escape == "u" and family != "byte":
        if cursor + 1 >= len(source) or source[cursor + 1] != "{":
            raise ValueError("invalid Unicode escape")
        end = source.find("}", cursor + 2)
        if end < 0:
            raise ValueError("unterminated Unicode escape")
        digits = source[cursor + 2:end].replace("_", "")
        if not 1 <= len(digits) <= 6 \
                or any(char not in "0123456789abcdefABCDEF" for char in digits):
            raise ValueError("invalid Unicode escape")
        scalar = int(digits, 16)
        if scalar > 0x10ffff or 0xd800 <= scalar <= 0xdfff:
            raise ValueError("invalid Unicode scalar escape")
        if family == "c" and scalar == 0:
            raise ValueError("NUL C string escape")
        return end + 1
    if continuation and escape in "\r\n":
        cursor += 1
        if escape == "\r" and cursor < len(source) and source[cursor] == "\n":
            cursor += 1
        while cursor < len(source) and source[cursor].isspace():
            cursor += 1
        return cursor
    raise ValueError("invalid escape")


def _skip_char(source, index, *, byte=False):
    end = _char_end(source, index, byte=byte)
    if end is None:
        raise ValueError("unterminated character literal")
    return end


def _char_end(source, index, *, byte=False):
    cursor = index + 1
    if cursor >= len(source):
        return None
    if source[cursor] == "\\":
        cursor = _skip_escape(source, cursor,
                              family="byte" if byte else "str")
    else:
        if source[cursor] in "\r\n'":
            return None
        if byte and ord(source[cursor]) > 0x7f:
            raise ValueError("non-ASCII byte character")
        cursor += 1
    if cursor < len(source) and source[cursor] == "'":
        return cursor + 1
    return None


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
        """Extract one unconditional public literal pin, failing closed."""
        try:
            with open(self.path(path), "rb") as handle:
                source = handle.read(RUST_SOURCE_CAP + 1)
        except OSError as err:
            self.fail_row("local-pin", f"{path}::{name}",
                          f"unreadable ({err})")
            return None
        if len(source) > RUST_SOURCE_CAP:
            self.fail_row("local-pin", f"{path}::{name}",
                          f"source exceeds {RUST_SOURCE_CAP} bytes")
            return None
        try:
            tokens = _rust_tokens(source.decode("utf-8"))
            if _has_conditional_crate_attribute(tokens):
                raise ValueError("conditional crate attribute")
            matches = _top_level_const_matches(tokens, name)
        except (UnicodeError, ValueError) as err:
            self.fail_row("local-pin", f"{path}::{name}",
                          f"unsupported Rust source ({err})")
            return None
        if len(matches) != 1:
            detail = (f"missing const {name}" if not matches
                      else f"duplicate const {name}")
            self.fail_row("local-pin", f"{path}::{name}", detail)
            return None
        index = matches[0]
        expected = [("punct", ":"), ("punct", "&"), ("ident", "str"),
                    ("punct", "="), None, ("punct", ";")]
        if index == 0 or tokens[index - 1] != ("ident", "pub"):
            self.fail_row("local-pin", f"{path}::{name}",
                          f"unsupported const {name} visibility")
            return None
        if index >= 2 and tokens[index - 2] == ("punct", "]"):
            self.fail_row("local-pin", f"{path}::{name}",
                          "unsupported authority attribute")
            return None
        actual = tokens[index + 2:index + 8]
        if len(actual) != len(expected) or any(
                want is not None and got != want
                for got, want in zip(actual, expected, strict=True)):
            self.fail_row("local-pin", f"{path}::{name}",
                          f"unsupported const {name} declaration")
            return None
        literal = actual[4]
        if literal[0] != "string" or literal[1] is None:
            self.fail_row("local-pin", f"{path}::{name}",
                          f"const {name} must use a plain string literal")
            return None
        return literal[1]

    def finish(self):
        if self.failures:
            print("check-freshness: FAIL", file=sys.stderr)
            for failure in self.failures:
                print(f"  - {failure}", file=sys.stderr)
            return 1
        print("check-freshness: PASS")
        return 0

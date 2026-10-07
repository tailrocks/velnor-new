#!/usr/bin/env python3
"""Validate commit trailers against the repository's canonical policy."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "docs/content/docs/implemented/codex-agent-configuration.mdx"
TRAILER_LABEL = re.compile(r"^(co-authored-by|signed-off-by)\b", re.IGNORECASE)
IDENTITY = re.compile(
    r"repository-local identity `(?P<name>[^`]+) <(?P<email>[^`]+)>`"
)
POLICY_HEADING = "## Commit identity and trailers"


class ValidationError(Exception):
    """An invalid policy, message, or Git identity."""


def canonical_values(text: str) -> tuple[list[str], tuple[str, str]]:
    identity_matches = list(IDENTITY.finditer(text))
    if len(identity_matches) != 1:
        raise ValidationError("canonical author identity is missing or ambiguous")
    identity_match = identity_matches[0]
    identity = (identity_match.group("name"), identity_match.group("email"))

    if text.splitlines().count(POLICY_HEADING) != 1:
        raise ValidationError("canonical trailer section is missing or ambiguous")
    section = text.split(POLICY_HEADING, 1)[1]
    blocks = re.findall(r"(?ms)^```text\n(.*?)^```$", section)
    if len(blocks) != 1:
        raise ValidationError("canonical trailer block is missing")
    expected = blocks[0].splitlines()
    expected_labels = ["Co-authored-by", "Signed-off-by"]
    labels = [line.partition(":")[0] for line in expected]
    if len(expected) != 2 or any(not line for line in expected) or labels != expected_labels:
        raise ValidationError("canonical trailer block must contain the two required lines")
    if expected[1] != f"Signed-off-by: {identity[0]} <{identity[1]}>":
        raise ValidationError("canonical trailers disagree with the documented identity")
    return expected, identity


def validate_message(message: str, expected: list[str]) -> None:
    lines = message.splitlines()
    trailer_lines = [line for line in lines if TRAILER_LABEL.match(line)]
    if trailer_lines != expected:
        raise ValidationError("message must contain each exact canonical trailer once")
    if len(lines) < 4 or lines[-2:] != expected or lines[-3] != "":
        raise ValidationError("canonical trailers must form the final, separated block")
    if not lines[0].strip():
        raise ValidationError("commit subject is missing")


def git_identity(kind: str) -> tuple[str, str]:
    result = subprocess.run(
        ["git", "var", f"GIT_{kind}_IDENT"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise ValidationError(f"cannot read Git {kind.lower()} identity")
    match = re.match(r"^(.*) <([^<>]+)> ", result.stdout.strip())
    if match is None:
        raise ValidationError(f"Git {kind.lower()} identity is malformed")
    return match.group(1), match.group(2)


def validate_local_identity(expected: tuple[str, str]) -> None:
    for kind in ("AUTHOR", "COMMITTER"):
        if git_identity(kind) != expected:
            raise ValidationError(f"Git {kind.lower()} identity differs from canonical policy")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("message_file", type=Path)
    parser.add_argument(
        "--check-local-identities",
        action="store_true",
        help="also verify the configured Git author and committer identities",
    )
    args = parser.parse_args()
    try:
        expected, identity = canonical_values(POLICY.read_text(encoding="utf-8"))
        validate_message(args.message_file.read_text(encoding="utf-8"), expected)
        if args.check_local_identities:
            validate_local_identity(identity)
    except (OSError, ValidationError) as error:
        print(f"commit trailer validation failed: {error}", file=sys.stderr)
        return 1
    print("commit message matches the canonical trailer policy")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

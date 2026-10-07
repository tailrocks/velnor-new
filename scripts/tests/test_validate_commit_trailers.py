import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
VALIDATOR = ROOT / "scripts/validate-commit-trailers.py"
SPEC = importlib.util.spec_from_file_location("trailer_validator", VALIDATOR)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
TRAILERS = (
    "Co-authored-by: Codex <codex@openai.com>\n"
    "Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>\n"
)


class CommitTrailerValidationTests(unittest.TestCase):
    def run_message(self, message: str) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            message_file = Path(directory) / "message.txt"
            message_file.write_text(message, encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(VALIDATOR), str(message_file)],
                check=False,
                capture_output=True,
                text=True,
            )

    def test_exact_trailer_block_with_final_newline_passes(self) -> None:
        result = self.run_message(f"change: keep provenance\n\nDetails.\n\n{TRAILERS}")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_trailer_fails(self) -> None:
        result = self.run_message("change: missing\n\nDetails.\n")
        self.assertNotEqual(result.returncode, 0)

    def test_short_signoff_name_fails(self) -> None:
        message = TRAILERS.replace("Alexey Zhokhov", "Alexey")
        result = self.run_message(f"change: short name\n\n{message}")
        self.assertNotEqual(result.returncode, 0)

    def test_wrong_email_fails(self) -> None:
        message = TRAILERS.replace("alexey@zhokhov.com", "wrong@example.com")
        result = self.run_message(f"change: wrong email\n\n{message}")
        self.assertNotEqual(result.returncode, 0)

    def test_wrong_codex_coauthor_fails(self) -> None:
        message = TRAILERS.replace("Codex <codex@openai.com>", "Codex <wrong@example.com>")
        result = self.run_message(f"change: wrong coauthor\n\n{message}")
        self.assertNotEqual(result.returncode, 0)

    def test_noncontiguous_trailers_fail(self) -> None:
        lines = TRAILERS.splitlines()
        message = f"change: split block\n\n{lines[0]}\nReviewed-by: Someone\n{lines[1]}\n"
        result = self.run_message(message)
        self.assertNotEqual(result.returncode, 0)

    def test_reversed_trailers_fail(self) -> None:
        lines = TRAILERS.splitlines()
        result = self.run_message(f"change: reversed\n\n{lines[1]}\n{lines[0]}\n")
        self.assertNotEqual(result.returncode, 0)

    def test_duplicate_trailers_fail(self) -> None:
        message = f"change: duplicate\n\n{TRAILERS}{TRAILERS}"
        result = self.run_message(message)
        self.assertNotEqual(result.returncode, 0)

    def test_body_decoy_fails(self) -> None:
        message = f"change: decoy\n\n{TRAILERS.splitlines()[1]}\n\n{TRAILERS}"
        result = self.run_message(message)
        self.assertNotEqual(result.returncode, 0)

    def test_trailing_blank_line_after_block_fails(self) -> None:
        result = self.run_message(f"change: trailing blank\n\n{TRAILERS}\n")
        self.assertNotEqual(result.returncode, 0)

    def test_ambiguous_policy_heading_fails_closed(self) -> None:
        policy = (ROOT / "docs/content/docs/implemented/codex-agent-configuration.mdx").read_text(
            encoding="utf-8"
        )
        with self.assertRaises(MODULE.ValidationError):
            MODULE.canonical_values(policy + "\n## Commit identity and trailers\n")

    def test_ambiguous_trailer_block_fails_closed(self) -> None:
        policy = (ROOT / "docs/content/docs/implemented/codex-agent-configuration.mdx").read_text(
            encoding="utf-8"
        )
        with self.assertRaises(MODULE.ValidationError):
            MODULE.canonical_values(policy + "\n```text\nextra\n```\n")

    def test_blank_line_inside_canonical_block_fails_closed(self) -> None:
        policy = (ROOT / "docs/content/docs/implemented/codex-agent-configuration.mdx").read_text(
            encoding="utf-8"
        )
        split_trailers = TRAILERS.replace("\nSigned-off-by:", "\n\nSigned-off-by:")
        malformed = policy.replace(TRAILERS, split_trailers, 1)
        with self.assertRaises(MODULE.ValidationError):
            MODULE.canonical_values(malformed)

    def test_local_author_and_committer_must_match_identity(self) -> None:
        ident = "Alexey Zhokhov <alexey@zhokhov.com> 1 +0000\n"
        result = subprocess.CompletedProcess(["git", "var"], 0, ident, "")
        with patch.object(MODULE.subprocess, "run", return_value=result):
            MODULE.validate_local_identity(("Alexey Zhokhov", "alexey@zhokhov.com"))

    def test_local_identity_mismatch_fails(self) -> None:
        ident = "Alexey <alexey@zhokhov.com> 1 +0000\n"
        result = subprocess.CompletedProcess(["git", "var"], 0, ident, "")
        with patch.object(MODULE.subprocess, "run", return_value=result):
            with self.assertRaises(MODULE.ValidationError):
                MODULE.validate_local_identity(("Alexey Zhokhov", "alexey@zhokhov.com"))


if __name__ == "__main__":
    unittest.main()

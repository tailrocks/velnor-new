"""Pinned parser regressions: pure strings; no Cargo, Git, or network."""
from pathlib import Path
import unittest


def require(condition, reason):
    if not condition:
        raise ValueError(reason)


NS = {"require": require, "__name__": "notes_test"}
SOURCE = Path(__file__).resolve().parents[1] / "src" / "release_prepare_notes.py"
exec(compile(SOURCE.read_text(), str(SOURCE), "exec"), NS)


class PreparationNotesTests(unittest.TestCase):
    def notes(self, text, version="1.2.3"):
        return NS["preparation_notes"](text.encode(), version)

    def test_latest_only_and_reference_definitions_preserved(self):
        text = ("# Changelog\n\n## [Unreleased]\n\n## [1.2.3](url) - date\n\n"
                "  - latest [link]\n\n[link]: https://example.invalid\n  \n"
                "## [1.2.2]\n\n- historical\n")
        self.assertEqual(self.notes(text), "  - latest [link]\n\n[link]: https://example.invalid")

    def test_fences_comments_and_lower_headings(self):
        body = ("### Added\n\n```markdown\n## 8.0.0\n```\n"
                "<!--\n## 9.0.0\n-->\n- actual")
        self.assertEqual(self.notes("## 1.2.3\n\n" + body + "\n## 1.2.2\nold"), body)

    def test_setext_and_indent(self):
        self.assertEqual(self.notes("Changelog\n===\n\nVersion [1.2.3]\n---\n\nnew\n\n1.2.2\n---\nold"), "new")
        self.assertEqual(self.notes("   ## v1.2.3\nnew\n    ## 9.0.0\nold"), "new\n    ## 9.0.0\nold")

    def test_nonrelease_and_higher_headings_terminate(self):
        for boundary in ("## Appendix", "# Appendix"):
            self.assertEqual(self.notes("## 1.2.3\nnew\n" + boundary + "\nexcluded"), "new")

    def test_unicode_whitespace_matches_rust_not_python(self):
        self.assertEqual(self.notes("## 1.2.3\n\u2003\nnew\u3000\n"), "new")
        self.assertEqual(self.notes("## 1.2.3\nnew\x1c\n"), "new\x1c")

    def test_crlf_preserved_in_body(self):
        self.assertEqual(self.notes("## 1.2.3\r\n\r\nfirst\r\nsecond\r\n\r\n## 1.2.2\r\nold"),
                         "first\r\nsecond")

    def test_empty_notes_and_final_heading(self):
        for text in ("## 1.2.3", "## 1.2.3\n", "## 1.2.3\n\n## 1.2.2\nold"):
            self.assertEqual(self.notes(text), "")

    def test_invalid_duplicate_and_wrong_version_fail(self):
        for text in ("# Changelog\nnone", "## unreleased\nnone", "## Unreleased\nnone",
                     "## 1.2.2\nold", "## 1.2.3\nnew\n## 1.2.2\nold\n## 1.2.2\nduplicate",
                     "## 1.2.3\n\x00"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                self.notes(text)

    def test_source_heading_quirks(self):
        self.assertEqual(self.notes("## [1.2.3] ###\nnew\n## 1.2.2\nold"), "new")
        self.assertEqual(self.notes("## 1.2.3\nnew\n~~~lang\n## 8.0.0\n~~~~suffix\nend"),
                         "new\n~~~lang\n## 8.0.0\n~~~~suffix\nend")


if __name__ == "__main__":
    unittest.main()

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("private-names-guard.py")
spec = importlib.util.spec_from_file_location("private_names_guard", SCRIPT)
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


def planted(*words):
    return frozenset(guard.digest(word.encode()) for word in words)


def repository(files):
    tmp = tempfile.TemporaryDirectory(prefix="private-names-test-")
    root = Path(tmp.name)
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    subprocess.run(["git", "init", "-q", str(root)], check=True, env=env)
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    subprocess.run(["git", "-C", str(root), "add", "-A"], check=True, env=env)
    return tmp, root


class ContentTests(unittest.TestCase):
    def test_planted_word_is_found_whole_inside_a_longer_name_and_in_a_pair(self):
        matcher = guard.Matcher(planted("zzplanted", "zzorg/zzrepo", "zz-two-parts"))
        for line, expected in [
            ("see ZZPlanted here", True),
            ("see zzplanted. next", True),
            ("rust-zzplanted-server", True),
            ("a_zzplanted_b", True),
            ("https://example.org/zzorg/zzrepo/pull/1", True),
            ("[zz-two-parts](x)", True),
            ("backend-zz-two-parts-x", True),
            ("zzorg / zzrepo", False),
            ("zzplantedx", False),
            ("zzrepo/zzorg", False),
            ("nothing to see", False),
        ]:
            self.assertEqual(len(matcher.flagged_lines(line.encode())) == 1, expected, line)

    def test_report_names_the_line_and_never_the_word(self):
        matcher = guard.Matcher(planted("zzplanted"))
        self.assertEqual(matcher.flagged_lines(b"one\ntwo zzplanted\nthree\nzzplanted again\n"), [2, 4])

    def test_committed_hashes_are_one_salted_digest_per_line(self):
        lines = guard.HASH_FILE.read_text().splitlines()
        self.assertTrue(lines)
        for line in lines:
            self.assertRegex(line, r"^[0-9a-f]{64}$")
        self.assertEqual(len(set(lines)), len(lines))


class PathTests(unittest.TestCase):
    def test_planted_file_name_fails_and_the_report_hides_the_word(self):
        tmp, root = repository({"docs/readme.md": "plain\n", "docs/zzplanted-notes.md": "plain\n"})
        with tmp:
            found = guard.scan(root, planted("zzplanted"))
            self.assertEqual(len(found), 1)
            self.assertNotIn("zzplanted", found[0])
            self.assertRegex(found[0], r"ls-files line 2\b")
            (root / "docs/zzplanted-notes.md").unlink()
            subprocess.run(["git", "-C", str(root), "add", "-A"], check=True)
            self.assertEqual(guard.scan(root, planted("zzplanted")), [])

    def test_planted_directory_name_and_directory_file_pair_fail(self):
        tmp, root = repository({"zzorg/zzrepo/notes.md": "plain\n", "docs/a/b.md": "plain\n"})
        with tmp:
            self.assertEqual(len(guard.scan(root, planted("zzorg"))), 1)
            self.assertEqual(len(guard.scan(root, planted("zzorg/zzrepo"))), 1)
            self.assertEqual(len(guard.scan(root, planted("a/b.md"))), 1)
            self.assertEqual(guard.scan(root, planted("zzrepo/zzorg")), [])

    def test_content_hit_in_a_clean_path_still_reports_path_and_line(self):
        tmp, root = repository({"docs/a.md": "one\nzzplanted\n"})
        with tmp:
            self.assertEqual(guard.scan(root, planted("zzplanted")), ["docs/a.md:2"])

    def test_content_hit_in_a_flagged_path_hides_the_path(self):
        tmp, root = repository({"zzplanted.md": "zzplanted\n"})
        with tmp:
            found = guard.scan(root, planted("zzplanted"))
            self.assertEqual(len(found), 2)
            self.assertTrue(all("zzplanted" not in item for item in found))

    def test_exempt_file_is_skipped(self):
        tmp, root = repository({"docs/hard-decisions.md": "zzplanted\n"})
        with tmp:
            self.assertEqual(guard.scan(root, planted("zzplanted")), [])


if __name__ == "__main__":
    unittest.main()

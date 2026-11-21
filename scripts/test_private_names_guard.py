# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import io
import json
import os
import random
import subprocess
import tempfile
import tracemalloc
import unittest
from contextlib import redirect_stdout
from pathlib import Path

SCRIPT = Path(__file__).with_name("private-names-guard.py")
spec = importlib.util.spec_from_file_location("private_names_guard", SCRIPT)
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)


def planted(*names):
    return dict((digest, int(length)) for digest, length in (guard.entry(name).split() for name in names))


def repository(files, links=None):
    tmp = tempfile.TemporaryDirectory(prefix="private-names-test-")
    root = Path(tmp.name)
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    subprocess.run(["git", "init", "-q", str(root)], check=True, env=env)
    for name, text in files.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    for name, target in (links or {}).items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.symlink_to(target)
    subprocess.run(["git", "-C", str(root), "add", "-A"], check=True, env=env)
    return tmp, root


def indexed(entries):
    tmp = tempfile.TemporaryDirectory(prefix="private-names-test-")
    root = Path(tmp.name)
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    subprocess.run(["git", "init", "-q", str(root)], check=True, env=env)
    for path, text in entries.items():
        obj = subprocess.run(
            ["git", "-C", str(root), "hash-object", "-w", "--stdin"], input=text, check=True, capture_output=True, env=env
        ).stdout.decode().strip()
        subprocess.run(
            ["git", "-C", str(root), "update-index", "--add", "--cacheinfo", f"100644,{obj},".encode() + path],
            check=True,
            env=env,
        )
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
            ("zzplantedx", True),
            ("zzplante", False),
            ("zzplant", False),
            ("zzrepo/zzorg", False),
            ("nothing to see", False),
        ]:
            self.assertEqual(len(matcher.flagged_lines(line.encode())) == 1, expected, line)

    def test_planted_name_embedded_in_an_identifier_is_found(self):
        matcher = guard.Matcher(planted("zzplanted", "zzrepo", "zz-two-parts"))
        for line, expected in [
            ("let client = ZzPlantedClient::new();", True),
            ("fn loadZzPlantedConfig()", True),
            ("fn load_zzplanted_config()", True),
            ("const LOAD_ZZPLANTED_CONFIG: u8 = 1;", True),
            ("loadzzplantedconfig", True),
            ("zzplantedzzrepo", True),
            ("ZzRepoZzPlanted", True),
            ("xzzrepoy", True),
            ("ZzTwoPartsClient", True),
            ("zz_two_parts", True),
            ("zz.two.parts", True),
            ("ZzPlantClient", False),
            ("zz two parts", False),
            ("zzrep0", False),
        ]:
            self.assertEqual(len(matcher.flagged_lines(line.encode())) == 1, expected, line)

    def test_parallel_search_finds_the_same_names_as_the_serial_one(self):
        text = b"one\ntwo zzplanted\n" + b"".join(b"filler%d x%dword\n" % (n, n) for n in range(300)) + b"zzplanted again\n"
        serial = guard.Matcher(planted("zzplanted")).flagged_lines(text)
        before = guard.PARALLEL_FROM
        guard.PARALLEL_FROM = 0
        try:
            self.assertEqual(guard.Matcher(planted("zzplanted")).flagged_lines(text), serial)
        finally:
            guard.PARALLEL_FROM = before
        self.assertEqual(serial, [2, 303])

    def test_report_names_the_line_and_never_the_word(self):
        matcher = guard.Matcher(planted("zzplanted"))
        self.assertEqual(matcher.flagged_lines(b"one\ntwo zzplanted\nthree\nzzplanted again\n"), [2, 4])

    def test_committed_hashes_are_a_salted_digest_and_a_length_per_line(self):
        lines = guard.HASH_FILE.read_text().splitlines()
        self.assertTrue(lines)
        for line in lines:
            self.assertRegex(line, r"^[0-9a-f]{64} [1-9][0-9]*$")
        digests = [line.split()[0] for line in lines]
        self.assertEqual(len(set(digests)), len(digests))
        self.assertEqual(len(guard.load_hashes()), len(lines))


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

    def test_planted_symlink_target_fails_and_the_report_hides_the_word(self):
        tmp, root = repository({"docs/a.md": "plain\n"}, {"docs/link": "../zzorg/zzrepo/notes.md"})
        with tmp:
            found = guard.scan(root, planted("zzorg/zzrepo"))
            self.assertEqual(found, ["docs/link:target"])
            self.assertEqual(len(guard.scan(root, planted("zzorg"))), 1)
            self.assertEqual(guard.scan(root, planted("zzother")), [])

    def test_planted_name_embedded_in_a_symlink_target_fails(self):
        tmp, root = repository({}, {"docs/link": "../LoadZzPlantedConfig.md"})
        with tmp:
            self.assertEqual(guard.scan(root, planted("zzplanted")), ["docs/link:target"])

    def test_symlink_in_a_flagged_path_hides_the_path(self):
        tmp, root = repository({}, {"zzplanted-link": "zzplanted"})
        with tmp:
            found = guard.scan(root, planted("zzplanted"))
            self.assertEqual(len(found), 2)
            self.assertTrue(all("zzplanted" not in item for item in found))

    def test_non_utf8_path_content_is_scanned_and_the_report_hides_the_word(self):
        tmp, root = indexed({b"docs/caf\xe9.md": b"one\nsee zzplanted\n", b"docs/plain.md": b"plain\n"})
        with tmp:
            found = guard.scan(root, planted("zzplanted"))
            self.assertEqual(len(found), 1)
            self.assertTrue(found[0].endswith(":2"), found)
            self.assertNotIn("zzplanted", found[0])
            self.assertEqual(guard.scan(root, planted("zzother")), [])

    def test_planted_name_in_a_non_utf8_path_fails(self):
        tmp, root = indexed({b"docs/zzplanted-\xff.md": b"plain\n"})
        with tmp:
            found = guard.scan(root, planted("zzplanted"))
            self.assertEqual(len(found), 1)
            self.assertNotIn("zzplanted", found[0])

    def test_binary_blob_is_skipped_and_text_blob_with_the_same_word_is_not(self):
        tmp, root = indexed({b"a.bin": b"zzplanted\0\x01", b"b.txt": b"zzplanted\n"})
        with tmp:
            self.assertEqual(guard.scan(root, planted("zzplanted")), ["b.txt:1"])

    def test_gitlink_whose_commit_is_absent_is_not_read_and_its_path_is_still_scanned(self):
        tmp, root = indexed({b"docs/a.md": b"plain\n"})
        with tmp:
            gitlink = f"160000,{'1' * 40},vendor/zzplanted-module".encode()
            subprocess.run(["git", "-C", str(root), "update-index", "--add", "--cacheinfo", gitlink], check=True)
            found = guard.scan(root, planted("zzplanted"))
            self.assertEqual(len(found), 1)
            self.assertNotIn("zzplanted", found[0])
            self.assertEqual(guard.scan(root, planted("zzother")), [])

    def test_name_in_hard_decisions_doc_is_found(self):
        tmp, root = repository({"docs/hard-decisions.md": "zzplanted\n"})
        with tmp:
            self.assertEqual(guard.scan(root, planted("zzplanted")), ["docs/hard-decisions.md:1"])


GIT_ENV = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}


def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True, env=GIT_ENV).stdout.decode().strip()


def pull_request(base_files, head_files, removed=()):
    tmp = tempfile.TemporaryDirectory(prefix="private-names-pr-")
    root = Path(tmp.name) / "work"
    root.mkdir()
    git(root, "init", "-q")
    git(root, "config", "user.email", "t@example.invalid")
    git(root, "config", "user.name", "t")
    for name, text in base_files.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(text)
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "base")
    base = git(root, "rev-parse", "HEAD")
    git(root, "remote", "add", "origin", str(root))
    for name, text in head_files.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(text)
    for name in removed:
        (root / name).unlink()
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "head")
    event = Path(tmp.name) / "event.json"
    event.write_text(json.dumps({"pull_request": {"base": {"sha": base}}}))
    return tmp, root, {"GITHUB_EVENT_NAME": "pull_request", "GITHUB_EVENT_PATH": str(event)}


def outcome(root, names, environ):
    out = io.StringIO()
    with redirect_stdout(out):
        code = guard.run(root, planted(*names), environ)
    return code, out.getvalue()


class PullRequestScopeTests(unittest.TestCase):
    def test_a_pull_request_scans_only_the_files_it_changes(self):
        tmp, root, environ = pull_request({"old.md": "zzplanted\n"}, {"new.md": "clean\n"})
        with tmp:
            self.assertEqual(outcome(root, ["zzplanted"], environ)[0], 0)
            self.assertEqual(outcome(root, ["zzplanted"], {})[0], 1)

    def test_a_pull_request_flags_a_planted_name_in_a_changed_file_and_a_changed_path(self):
        tmp, root, environ = pull_request({"old.md": "clean\n"}, {"new.md": "one\nzzplanted\n", "zzplanted-path.md": "x\n"})
        with tmp:
            code, text = outcome(root, ["zzplanted"], environ)
            self.assertEqual(code, 1)
            self.assertIn("new.md:2", text)
            self.assertIn("2 finding(s)", text)
            self.assertIn("ls-files line 3", text)
            self.assertNotIn("zzplanted", text)

    def test_a_pull_request_that_deletes_a_file_still_passes(self):
        tmp, root, environ = pull_request({"old.md": "zzplanted\n", "keep.md": "clean\n"}, {}, removed=["old.md"])
        with tmp:
            self.assertEqual(outcome(root, ["zzplanted"], environ)[0], 0)

    def test_an_unreadable_base_falls_back_to_the_whole_tree(self):
        tmp, root, environ = pull_request({"old.md": "zzplanted\n"}, {"new.md": "clean\n"})
        with tmp:
            Path(environ["GITHUB_EVENT_PATH"]).write_text(json.dumps({"pull_request": {"base": {"sha": "0" * 40}}}))
            self.assertEqual(outcome(root, ["zzplanted"], environ)[0], 1)
            Path(environ["GITHUB_EVENT_PATH"]).write_text("not json")
            self.assertEqual(outcome(root, ["zzplanted"], environ)[0], 1)

    def test_push_and_schedule_events_scan_the_whole_tree(self):
        tmp, root, environ = pull_request({"old.md": "zzplanted\n"}, {"new.md": "clean\n"})
        with tmp:
            for name in ["push", "schedule", "merge_group", "workflow_dispatch"]:
                self.assertEqual(outcome(root, ["zzplanted"], {**environ, "GITHUB_EVENT_NAME": name})[0], 1, name)


class LongTokenTests(unittest.TestCase):
    def test_one_long_token_is_scanned_without_materializing_its_windows(self):
        token = "".join(random.Random(1).choices("abcdefghijklmnopqrstuvwxyz0123456789", k=100_000)).encode()
        matcher = guard.Matcher(planted("zzplanted", "zz-two-parts"))
        tracemalloc.start()
        try:
            found = matcher.names_in([token + b"zzplanted" + token])
            peak = tracemalloc.get_traced_memory()[1]
        finally:
            tracemalloc.stop()
        self.assertEqual(found, {b"zzplanted"})
        self.assertLess(peak, 2_000_000)


if __name__ == "__main__":
    unittest.main()

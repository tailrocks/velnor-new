"""Offline exact Git tree/blob authority regression proofs."""
import base64
import hashlib
from pathlib import Path
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "src" / "release_source_tree.py"
COMMIT = "a" * 40
REPOSITORY = "owner/repository"


class SourceError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise SourceError(message)


def git_hash(kind, raw):
    return hashlib.sha1(kind + b" " + str(len(raw)).encode() + b"\0" + raw).hexdigest()


def fixture(files):
    entries, blobs = {}, {}
    for path, raw in files.items():
        sha = git_hash(b"blob", raw)
        entries[path] = {"path": path, "type": "blob", "mode": "100644",
                         "sha": sha, "size": len(raw)}
        blobs[sha] = {"sha": sha, "encoding": "base64", "size": len(raw),
                      "content": base64.b64encode(raw).decode()}
        parent = path.rpartition("/")[0]
        while parent:
            entries.setdefault(parent, {"path": parent, "type": "tree", "mode": "040000"})
            parent = parent.rpartition("/")[0]
    directories = ["", *(path for path, item in entries.items() if item["type"] == "tree")]
    root_sha = None
    for directory in sorted(directories, key=lambda path: path.count("/") + bool(path), reverse=True):
        children = [(path.rpartition("/")[2].encode(), item)
                    for path, item in entries.items() if path.rpartition("/")[0] == directory]
        children.sort(key=lambda pair: pair[0] + (b"/" if pair[1]["type"] == "tree" else b""))
        raw = b"".join((b"40000" if item["type"] == "tree" else item["mode"].encode()) +
                       b" " + name + b"\0" + bytes.fromhex(item["sha"])
                       for name, item in children)
        sha = git_hash(b"tree", raw)
        if directory:
            entries[directory]["sha"] = sha
        else:
            root_sha = sha
    return {"sha": COMMIT, "tree": {"sha": root_sha}}, {
        "sha": root_sha, "truncated": False, "tree": list(entries.values())}, blobs


class SourceTreeTest(unittest.TestCase):
    def setUp(self):
        self.commit, self.tree, self.blobs = fixture({
            "Cargo.toml": b"[package]\nname='one'\nversion='1.0.0'\n",
            "foo/lib.rs": b"pub fn hello() {}\n", "foo.bar": b"ordering",
            "CHANGELOG.md": b"# 1.0.0\n\nNotes\n", "empty": b""})
        self.calls = []
        self.ns = {"require": require, "ReconcileError": SourceError,
                   "read_source_commit": self.read_commit,
                   "read_source_tree": self.read_tree, "read_source_blob": self.read_blob}
        exec(compile(SOURCE.read_bytes(), str(SOURCE), "exec"), self.ns)

    def read_commit(self, repository, sha):
        self.assertEqual((repository, sha), (REPOSITORY, COMMIT))
        self.calls.append(("commit", sha))
        return self.commit

    def read_tree(self, repository, sha):
        self.assertEqual((repository, sha), (REPOSITORY, self.commit["tree"]["sha"]))
        self.calls.append(("tree", sha))
        return self.tree

    def read_blob(self, repository, sha):
        self.assertEqual(repository, REPOSITORY)
        self.calls.append(("blob", sha))
        return self.blobs[sha]

    def source(self):
        return self.ns["source_tree"]({"repository": REPOSITORY, "source_sha": COMMIT})

    def test_exact_source_and_cached_blob(self):
        source = self.source()
        self.assertEqual(source.repository, REPOSITORY)
        self.assertEqual(source.source_sha, COMMIT)
        read = self.ns["source_file"]
        self.assertEqual(read(source, "CHANGELOG.md"), b"# 1.0.0\n\nNotes\n")
        self.assertEqual(read(source, "CHANGELOG.md"), b"# 1.0.0\n\nNotes\n")
        self.assertEqual(read(source, "empty", 0), b"")
        self.assertEqual(sum(kind == "blob" for kind, _ in self.calls), 2)

    def test_source_immutable_and_forged_authority_rejected(self):
        source = self.source()
        with self.assertRaises(AttributeError):
            source.source_sha = "b" * 40
        with self.assertRaises(TypeError):
            source.entries["Cargo.toml"]["sha"] = "b" * 40
        with self.assertRaises(SourceError):
            self.ns["source_file"]({"entries": source.entries}, "Cargo.toml")
        with self.assertRaises(SourceError):
            self.ns["source_blob"](source, dict(source.entries["Cargo.toml"]), 1024)
        with self.assertRaises(SourceError):
            self.ns["_SourceTree"](None, REPOSITORY, COMMIT, source.tree_sha, {})

    def test_reject_unsafe_paths(self):
        paths = ["/absolute", "a//b", "a/../b", "./b", "a\\b", ".git/config",
                 "a/.GIT/config", "a\0b", "a\nb", "a\x7fb", "a\x85b", "bad\ud800"]
        for path in paths:
            with self.subTest(path=repr(path)):
                self.tree["tree"][0]["path"] = path
                with self.assertRaises(SourceError):
                    self.source()

    def test_reject_symlink_gitlink_wrong_mode(self):
        for mode, kind in [("120000", "blob"), ("160000", "commit"),
                           ("040000", "blob"), ("100644", "tree")]:
            with self.subTest(mode=mode, kind=kind):
                self.tree["tree"][0].update(mode=mode, type=kind)
                with self.assertRaises(SourceError):
                    self.source()

    def test_reject_duplicate_parent_collision_truncation_and_digest(self):
        original = list(self.tree["tree"])
        mutations = [original + [original[0]],
                     [item for item in original if item["path"] != "foo"],
                     [{**item, "type": "blob", "mode": "100644", "size": 0}
                      if item["path"] == "foo" else item for item in original],
                     [{**item, "sha": "b" * 40} if item["path"] == "foo" else item
                      for item in original]]
        for entries in mutations:
            self.tree["tree"] = entries
            with self.assertRaises(SourceError):
                self.source()
        self.tree["tree"] = original
        self.tree["truncated"] = True
        with self.assertRaises(SourceError):
            self.source()

    def test_reject_wrong_commit_tree_identity(self):
        self.commit["sha"] = "b" * 40
        with self.assertRaises(SourceError):
            self.source()
        self.commit["sha"] = COMMIT
        self.tree["sha"] = "b" * 40
        with self.assertRaises(SourceError):
            self.source()

    def test_reject_blob_digest_size_encoding_and_bool_limit(self):
        original = self.blobs[self.tree["tree"][0]["sha"]]
        mutations = [{**original, "content": base64.b64encode(b"x" * original["size"]).decode()},
                     {**original, "size": True}, {**original, "size": 0},
                     {**original, "encoding": "utf8"}, {**original, "content": "!"},
                     {**original, "sha": "b" * 40}]
        for blob in mutations:
            self.blobs[original["sha"]] = blob
            with self.assertRaises(SourceError):
                self.ns["source_file"](self.source(), "Cargo.toml")
        self.blobs[original["sha"]] = original
        source = self.source()
        for limit in (True, -1, 17 * 1024 * 1024, 0):
            with self.assertRaises(SourceError):
                self.ns["source_file"](source, "Cargo.toml", limit)

    def test_empty_tree(self):
        self.commit, self.tree, self.blobs = fixture({})
        self.assertEqual(self.tree["sha"], "4b825dc642cb6eb9a060e54bf8d69288fbee4904")
        self.assertEqual(dict(self.source().entries), {})

    def test_executable_and_nested_empty_tree(self):
        empty_blob = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        empty_tree = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        raw = b"40000 empty\0" + bytes.fromhex(empty_tree)
        raw += b"100755 run\0" + bytes.fromhex(empty_blob)
        root = git_hash(b"tree", raw)
        self.commit = {"sha": COMMIT, "tree": {"sha": root}}
        self.tree = {"sha": root, "truncated": False, "tree": [
            {"path": "run", "sha": empty_blob, "type": "blob", "mode": "100755", "size": 0},
            {"path": "empty", "sha": empty_tree, "type": "tree", "mode": "040000"}]}
        self.blobs = {empty_blob: {"sha": empty_blob, "encoding": "base64", "size": 0,
                                   "content": ""}}
        source = self.source()
        self.assertEqual(self.ns["source_file"](source, "run", 0), b"")
        self.assertEqual(source.entries["run"]["mode"], "100755")

    def test_total_size_and_entry_count_bounds(self):
        self.commit, self.tree, self.blobs = fixture({str(index): b"" for index in range(9)})
        for entry in self.tree["tree"]:
            entry["size"] = 16 * 1024 * 1024
        with self.assertRaisesRegex(SourceError, "source_tree_total_size"):
            self.source()
        self.tree["tree"] = [self.tree["tree"][0]] * 20001
        with self.assertRaisesRegex(SourceError, "source_tree_response"):
            self.source()

    def test_strict_entry_size_and_mode_types(self):
        item = self.tree["tree"][0]
        for size in (True, -1, "1", 17 * 1024 * 1024):
            item["size"] = size
            with self.assertRaises(SourceError):
                self.source()
        item["size"] = 40
        for key in ("mode", "type"):
            saved = item[key]
            item[key] = []
            with self.assertRaises(SourceError):
                self.source()
            item[key] = saved

    def test_cached_same_sha_conflicting_size_rejected(self):
        self.commit, self.tree, self.blobs = fixture({"first": b"same", "second": b"same"})
        self.tree["tree"][1]["size"] = 5
        source = self.source()
        read = self.ns["source_file"]
        self.assertEqual(read(source, "first"), b"same")
        with self.assertRaises(SourceError):
            read(source, "second")

    def test_cached_blob_corruption_rejected(self):
        source = self.source()
        read = self.ns["source_file"]
        raw = read(source, "CHANGELOG.md")
        sha = source.entries["CHANGELOG.md"]["sha"]
        for replacement in (b"x" * len(raw), "x" * len(raw), bytearray(raw)):
            source._blobs[sha] = replacement
            with self.assertRaises(SourceError):
                read(source, "CHANGELOG.md")

    def test_non_utf8_blob_preserved_and_missing_directory_rejected(self):
        self.commit, self.tree, self.blobs = fixture({"a/b/c": b"\xff\0\xfe"})
        source = self.source()
        self.assertEqual(self.ns["source_file"](source, "a/b/c"), b"\xff\0\xfe")
        for path in ("absent", "a"):
            with self.assertRaises(SourceError):
                self.ns["source_file"](source, path)


if __name__ == "__main__":
    unittest.main()

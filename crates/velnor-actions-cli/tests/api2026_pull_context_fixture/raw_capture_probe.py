# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import pathlib
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import raw_capture


def git(root, *args):
    return subprocess.run(["/usr/bin/git", "-C", str(root), *args], check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout.strip()


def main():
    with tempfile.TemporaryDirectory() as temporary:
        base = pathlib.Path(temporary)
        root, destination = base / "source", base / "capture"
        root.mkdir(); destination.mkdir()
        git(root, "init", "--quiet")
        git(root, "config", "user.name", "Alexey Zhokhov")
        git(root, "config", "user.email", "alexey@zhokhov.com")
        git(root, "remote", "add", "origin", "https://github.com/tailrocks/velnor-new.git")
        (root / "docs").mkdir(); (root / "docs/item").write_text("head")
        (root / "source").write_text("head source")
        (root / "link").symlink_to("docs/item")
        git(root, "add", "."); git(root, "commit", "-s", "-m", "fixture")
        git(root, "update-index", "--info-only", "--add", "--cacheinfo", "160000,1111111111111111111111111111111111111111,submodule")
        (root / "submodule").mkdir()
        (root / "docs/item").write_text("staged")
        git(root, "add", "docs/item")
        staged_oid = git(root, "rev-parse", ":docs/item").decode()
        assert staged_oid != git(root, "rev-parse", "HEAD:docs/item").decode()
        alternate = base / "alternate-objects"
        (alternate / staged_oid[:2]).mkdir(parents=True)
        indexed_object = root / ".git/objects" / staged_oid[:2] / staged_oid[2:]
        missing_object = alternate / staged_oid[:2] / staged_oid[2:]
        indexed_object.rename(missing_object)
        (root / ".git/objects/info/alternates").write_text(str(alternate) + "\n")
        (root / "source").write_text("dirty source")
        (root / "docs/item").unlink()
        git(root, "update-index", "--skip-worktree", "docs/item")
        result = raw_capture.capture(root, destination, base / "manifest.json")
        assert (destination / "docs/item").read_text() == "staged"
        assert (destination / "source").read_text() == "dirty source"
        assert (destination / "link").is_symlink()
        hydrated = [entry for entry in result["entries"] if entry["hydrated"]]
        assert len(hydrated) == 1 and hydrated[0]["oid"] == staged_oid
        assert (destination / "submodule").is_dir() and not any((destination / "submodule").iterdir())
        (root / "source").write_text("later")
        assert (destination / "source").read_text() == "dirty source"
        assert not (destination / ".git/objects/info/alternates").exists()
        assert git(destination, "ls-files", "--stage", "-z") == git(root, "ls-files", "--stage", "-z")
        (root / "submodule/content").write_text("initialized")
        initialized = base / "initialized"; initialized.mkdir()
        try:
            raw_capture.capture(root, initialized, base / "initialized-manifest.json")
        except RuntimeError:
            pass
        else:
            raise AssertionError("initialized gitlink accepted")
        (root / "submodule/content").unlink()
        projected = base / "projected"; projected.mkdir()
        try:
            raw_capture.project(destination, projected, result, [])
        except RuntimeError:
            pass
        else:
            raise AssertionError("unreviewed skip accepted")
        try:
            raw_capture.project(destination, projected, result, ["docs/**"])
        except RuntimeError:
            pass
        else:
            raise AssertionError("projection accepted a link to omitted source")
        git(root, "rm", "--cached", "link"); (root / "link").unlink()
        clean_capture = base / "clean-capture"; clean_capture.mkdir()
        clean = raw_capture.capture(root, clean_capture, base / "clean-manifest.json")
        clean_projection = base / "clean-projection"; clean_projection.mkdir()
        result = raw_capture.project(clean_capture, clean_projection, clean, ["docs/**"])
        assert b"docs/item" not in result and not (clean_projection / "docs/item").exists()
        assert (clean_projection / "source").read_text() == "later"
        assert (clean_projection / "submodule").is_dir()
        missing_object.unlink()
        missing_capture = base / "missing-capture"; missing_capture.mkdir()
        try:
            raw_capture.capture(root, missing_capture, base / "missing-manifest.json")
        except RuntimeError:
            pass
        else:
            raise AssertionError("missing indexed object accepted")


if __name__ == "__main__":
    main()

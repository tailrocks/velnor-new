# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import pathlib
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import driver
import source_snapshot


def git(root, *args):
    subprocess.run(["/usr/bin/git", "-C", str(root), *args], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def main():
    with tempfile.TemporaryDirectory() as temporary:
        root = pathlib.Path(temporary)
        git(root, "init")
        git(root, "config", "user.name", "Alexey Zhokhov")
        git(root, "config", "user.email", "alexey@zhokhov.com")
        (root / "source").write_text("reviewed")
        (root / "docs").mkdir()
        (root / "docs/first").write_text("first")
        git(root, "add", ".")
        git(root, "commit", "-s", "-m", "fixture")
        git(root, "sparse-checkout", "set", "--no-cone", "/*", "!/docs/**")
        patterns = ["docs/**"]
        before = source_snapshot.collect(root, None, driver.read_tracked, patterns)
        assert not (root / "docs/first").exists()
        git(root, "update-index", "--force-remove", "--", "docs/first")
        after = source_snapshot.collect(root, None, driver.read_tracked, patterns)
        assert source_snapshot.digest(before) == source_snapshot.digest(after)
        oid = subprocess.check_output(["/usr/bin/git", "-C", str(root), "rev-parse", "HEAD:docs/first"]).decode().strip()
        git(root, "update-index", "--add", "--cacheinfo", "100644," + oid + ",docs/second")
        git(root, "update-index", "--skip-worktree", "docs/second")
        added = source_snapshot.collect(root, None, driver.read_tracked, patterns)
        assert source_snapshot.digest(before) == source_snapshot.digest(added)
        (root / "source").unlink()
        git(root, "update-index", "--skip-worktree", "source")
        try:
            source_snapshot.collect(root, None, driver.read_tracked, patterns)
        except RuntimeError:
            pass
        else:
            raise AssertionError("unreviewed skip-worktree accepted")
        git(root, "update-index", "--no-skip-worktree", "source")
        (root / "source").write_text("reviewed")
        (root / "docs").mkdir(exist_ok=True)
        (root / "docs/second").write_text("present")
        present = source_snapshot.collect(root, None, driver.read_tracked, patterns)
        assert b"docs/second" in present
        assert source_snapshot.digest(present) != source_snapshot.digest(before)
        (root / "docs/second").unlink()
        (root / "docs").rmdir()
        git(root, "update-index", "--skip-worktree", "docs/second")
        with tempfile.TemporaryDirectory() as outside:
            (root / "docs").symlink_to(outside)
            try:
                source_snapshot.excluded_absent(root, b"docs/second")
            except OSError:
                pass
            else:
                raise AssertionError("excluded symlink parent accepted")
            (root / "docs").unlink()
        (root / "source").unlink()
        try:
            source_snapshot.collect(root, None, driver.read_tracked, patterns)
        except RuntimeError:
            pass
        else:
            raise AssertionError("unexpected missing source accepted")
        (root / "source").write_text("reviewed")
        with tempfile.TemporaryDirectory() as external:
            marker = pathlib.Path(external) / "invoked"
            hook = pathlib.Path(external) / "fsmonitor"
            hook.write_text("#!/bin/sh\n: > '" + str(marker) + "'\n")
            hook.chmod(0o755)
            git(root, "config", "core.fsmonitor", str(hook))
            source_snapshot.collect(root, None, driver.read_tracked, patterns)
            assert not marker.exists(), "fixture enumeration executed local fsmonitor"


if __name__ == "__main__":
    main()

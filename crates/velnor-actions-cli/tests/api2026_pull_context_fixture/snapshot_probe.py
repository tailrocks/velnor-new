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
    subprocess.run(["/usr/bin/git", "-C", str(root), *args], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def refuses(action):
    try:
        action()
    except RuntimeError:
        return
    raise AssertionError("unsafe or changed fixture input was accepted")


def main():
    with tempfile.TemporaryDirectory() as temporary:
        root = pathlib.Path(temporary) / "source"
        snapshot = pathlib.Path(temporary) / "snapshot"
        root.mkdir(); snapshot.mkdir()
        git(root, "init")
        (root / ".gitignore").write_text("ignored/\n")
        (root / "tracked").write_text("base")
        (root / "tracked").chmod(0o755)
        (root / "folder").mkdir()
        (root / "folder/file").write_text("member")
        (root / "safe").symlink_to("folder")
        (root / "chain").symlink_to("safe/file")
        git(root, "add", ".gitignore", "tracked", "folder", "safe", "chain")
        (root / "tracked").write_text("dirty")
        (root / "ignored").mkdir()
        (root / "ignored/private").write_text("synthetic ignored data")
        entries = source_snapshot.collect(root, snapshot, driver.read_tracked)
        driver.verify_sources(root, snapshot, entries)
        assert (snapshot / "tracked").read_text() == "dirty"
        assert (snapshot / "tracked").stat().st_mode & 0o777 == 0o755
        assert (snapshot / "chain").read_text() == "member"
        assert not (snapshot / "ignored").exists() and not (snapshot / ".git").exists()
        (snapshot / "extra").write_text("extra")
        refuses(lambda: source_snapshot.verify_snapshot(snapshot, entries, driver.read_tracked))
        (snapshot / "extra").unlink()
        (snapshot / "tracked").write_text("tampered")
        refuses(lambda: driver.verify_sources(root, snapshot, entries))
        (snapshot / "tracked").write_text("dirty")
        (snapshot / "tracked").unlink()
        refuses(lambda: source_snapshot.verify_snapshot(snapshot, entries, driver.read_tracked))
        (snapshot / "tracked").write_text("dirty")
        (snapshot / "tracked").chmod(0o755)
        (root / "tracked").write_text("changed")
        changed = source_snapshot.collect(root, None, driver.read_tracked)
        assert source_snapshot.digest(changed) != source_snapshot.digest(entries)
        refuses(lambda: driver.verify_sources(root, snapshot, entries))
        (root / "untracked").write_text("synthetic untracked data")
        refuses(lambda: source_snapshot.collect(root, None, driver.read_tracked))
        (root / "untracked").unlink()
        (root / "escape").symlink_to("../outside")
        git(root, "add", "escape")
        refuses(lambda: source_snapshot.collect(root, None, driver.read_tracked))
        git(root, "rm", "--cached", "escape"); (root / "escape").unlink()
        (root / "a").symlink_to("b"); (root / "b").symlink_to("a")
        git(root, "add", "a", "b")
        refuses(lambda: source_snapshot.collect(root, None, driver.read_tracked))
        (root / "a").unlink(); (root / "a").symlink_to("ignored/private")
        refuses(lambda: source_snapshot.collect(root, None, driver.read_tracked))
        git(root, "rm", "-f", "--cached", "a", "b")
        (root / "a").unlink(); (root / "b").unlink()
        (root / "ancestor").symlink_to(".")
        git(root, "add", "ancestor")
        refuses(lambda: source_snapshot.collect(root, None, driver.read_tracked))
        git(root, "rm", "--cached", "ancestor"); (root / "ancestor").unlink()
        for name in ("left", "right"):
            (root / name).mkdir(); (root / name / "file").write_text(name)
        (root / "left/link").symlink_to("../right")
        (root / "right/link").symlink_to("../left")
        git(root, "add", "left", "right")
        refuses(lambda: source_snapshot.collect(root, None, driver.read_tracked))
        long_root = pathlib.Path(temporary) / "long"
        long_root.mkdir(); git(long_root, "init")
        (long_root / "target").write_text("tracked")
        for index in range(21):
            target = f"link{index + 1:02}" if index < 20 else "target"
            (long_root / f"link{index:02}").symlink_to("./" * 400 + target)
        git(long_root, "add", ".")
        refuses(lambda: source_snapshot.collect(long_root, None, driver.read_tracked))


if __name__ == "__main__":
    main()

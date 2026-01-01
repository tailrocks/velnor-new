"""Read-only provenance checks for the private owned-source Git fixture."""

import os
from pathlib import Path
import shutil
import subprocess


def fixture_ignored_blob(case, directory, tree):
    directory = directory.resolve(strict=True)
    metadata = directory / ".git"
    case.assertEqual(metadata.resolve(strict=True), metadata)
    case.assertTrue(metadata.is_dir())
    executable = shutil.which("git", path=os.defpath)
    case.assertIsNotNone(executable)
    env = {"PATH": os.defpath, "LC_ALL": "C", "HOME": str(directory.parent),
           "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_SYSTEM": os.devnull,
           "GIT_CONFIG_GLOBAL": os.devnull, "GIT_ATTR_NOSYSTEM": "1",
           "GIT_TERMINAL_PROMPT": "0"}
    argv = [str(Path(executable).resolve(strict=True)), "--no-replace-objects",
            "--git-dir=" + str(metadata), "--work-tree=" + str(directory),
            "-C", str(directory), "-c", "core.hooksPath=" + os.devnull,
            "-c", "core.fsmonitor=false", "-c", "safe.directory=" + str(directory)]

    def read(*arguments, check=True):
        return subprocess.run([*argv, *arguments], env=env, cwd=directory,
                              check=check, capture_output=True)

    for argument, expected in (("--absolute-git-dir", metadata),
                               ("--git-common-dir", metadata),
                               ("--show-toplevel", directory)):
        actual = read("rev-parse", argument).stdout.decode().strip()
        case.assertEqual(Path(actual).resolve(strict=True), expected)
    head = read("rev-parse", "--verify", "HEAD", check=False)
    case.assertEqual(head.returncode, 128)
    case.assertEqual(head.stdout, b"")
    case.assertTrue(read("symbolic-ref", "HEAD").stdout.startswith(b"refs/heads/"))
    case.assertRegex(tree, r"^[a-f0-9]{40}$")
    case.assertEqual(read("rev-parse", tree + "^{tree}").stdout.strip(), tree.encode())
    entries = read("ls-tree", "-r", "-z", tree).stdout.split(b"\0")
    staged = []
    for entry in entries[:-1]:
        identity, name = entry.split(b"\t", 1)
        mode, kind, oid = identity.split(b" ")
        case.assertIn(kind, (b"blob", b"commit"))
        staged.append(mode + b" " + oid + b" 0\t" + name + b"\0")
    case.assertEqual(read("ls-files", "--stage", "-z").stdout, b"".join(staged))
    entry = read("ls-tree", "-z", tree, "--", "ignored").stdout
    blob = read("rev-parse", tree + ":ignored").stdout.strip()
    case.assertEqual(entry, b"100755 blob " + blob + b"\tignored\0")
    case.assertEqual(read("rev-parse", ":ignored").stdout.strip(), blob)
    return read("show", ":ignored").stdout


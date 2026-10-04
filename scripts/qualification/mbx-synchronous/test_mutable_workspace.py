"""Writable fixture copies use the checked-in input binder and source bytes."""

import importlib.util
import json
from pathlib import Path
import shutil
import stat
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("mutable_workspace_inputs", ROOT / "bind_inputs.py")
BIND = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BIND)


def physical(root):
    records = []
    for path in [root, *sorted(root.rglob("*"))]:
        if path.is_symlink():
            raise AssertionError("unexpected fixture symlink")
        info = path.stat()
        digest = BIND.sha(path.read_bytes()) if path.is_file() else None
        records.append((path.relative_to(root).as_posix(), stat.S_IMODE(info.st_mode),
                        digest, info.st_ino))
    return records


class MutableWorkspaceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.root = Path(temporary.name).resolve()

        def cleanup():
            for path in self.root.rglob("*"):
                path.chmod(0o700 if path.is_dir() else 0o600)
            temporary.cleanup()

        self.addCleanup(cleanup)
        manifest = json.loads((ROOT / "manifest.json").read_bytes())
        reviewed = BIND.fixture_source(manifest)
        self.expected = manifest["fixture"]["files"]
        self.source = self.root / "readonly-source"
        shutil.copytree(reviewed, self.source)
        for path in [self.source, *self.source.rglob("*")]:
            path.chmod(0o500 if path.is_dir() else 0o400)
        self.before = physical(self.source)

    def test_copy_fixture_normalizes_readonly_source_before_lock_materialization(self):
        workspace = self.root / "mutable-workspace"
        BIND.copy_fixture(self.source, workspace)
        self.assertEqual(BIND.fixture_inventory(workspace, self.expected), self.expected)
        for path in [workspace, *workspace.rglob("*")]:
            self.assertEqual(stat.S_IMODE(path.stat().st_mode),
                             0o700 if path.is_dir() else 0o600, str(path))
        for relative in ("Cargo.toml", "Cargo.lock", "src/lib.rs"):
            path = workspace / relative
            original = path.read_bytes()
            path.write_bytes(original)
            self.assertEqual(path.read_bytes(), original)
        self.assertEqual(physical(self.source), self.before)
        shutil.rmtree(workspace)
        self.assertFalse(workspace.exists())
        self.assertEqual(physical(self.source), self.before)

    def test_copytree_reproduces_readonly_permission_failure(self):
        copied = self.root / "old-copytree"
        shutil.copytree(self.source, copied)
        self.assertEqual(stat.S_IMODE(copied.stat().st_mode), 0o500)
        self.assertEqual(stat.S_IMODE((copied / "src/lib.rs").stat().st_mode), 0o400)
        with self.assertRaises(PermissionError):
            (copied / "src/lib.rs").write_bytes(b"ordinary source change")
        self.assertEqual(physical(self.source), self.before)


if __name__ == "__main__":
    unittest.main()

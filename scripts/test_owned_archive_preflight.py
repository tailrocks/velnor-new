"""Verify archive preflight stays bound to its trusted checkout executable."""

import importlib.util
import os
from pathlib import Path
import shutil
import stat
import tempfile
import unittest
from unittest.mock import patch


MODULE_PATH = Path(__file__).with_name("owned_archive_preflight.py")
MODULE_SPEC = importlib.util.spec_from_file_location("archive_preflight_test", MODULE_PATH)
if MODULE_SPEC is None or MODULE_SPEC.loader is None:
    raise RuntimeError("archive preflight module unavailable")
PRECHECK = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(PRECHECK)


def checkout(root):
    scripts = root / "scripts"
    scripts.mkdir(parents=True)
    shutil.copyfile(MODULE_PATH, scripts / MODULE_PATH.name)
    shutil.copyfile(MODULE_PATH.with_name("archive-guard-inputs.txt"),
                    scripts / "archive-guard-inputs.txt")
    (root / ".git").mkdir()
    manifest = (scripts / "archive-guard-inputs.txt").read_text(encoding="ascii")
    for line in manifest.splitlines():
        kind, relative = line.split(" ")
        path = root / relative
        if kind == "optional":
            continue
        if relative == "scripts/owned_archive_preflight.py":
            # The script under test: keep the valid copy installed above.
            continue
        if kind == "tree":
            path.mkdir(parents=True, exist_ok=True)
            (path / "source.rs").write_text("mod new_module;\n", encoding="utf-8")
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("guard source fixture\n", encoding="utf-8")
    executable = root / ".velnor/archive-guard/bin/velnor-archive-guard"
    executable.parent.mkdir(parents=True)
    executable.write_bytes(b"native executable fixture")
    executable.chmod(0o755)
    return executable, scripts / MODULE_PATH.name


class ArchivePreflightTests(unittest.TestCase):
    def test_fixed_checkout_executable_is_selected(self):
        with tempfile.TemporaryDirectory() as temporary:
            temporary = os.path.realpath(temporary)
            expected, module_path = checkout(Path(temporary))
            spec = importlib.util.spec_from_file_location("checkout_guard", module_path)
            self.assertIsNotNone(spec)
            self.assertIsNotNone(spec.loader)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            fingerprint = module._source_fingerprint(Path(temporary))
            result = type("Result", (), {"returncode": 0,
                "stdout": (fingerprint + "\n").encode(), "stderr": b""})()
            with patch.object(module.subprocess, "run", return_value=result):
                self.assertEqual(module._guard_path(), expected)

    def test_missing_or_symlink_executable_fails_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            temporary = os.path.realpath(temporary)
            executable, module_path = checkout(Path(temporary))
            spec = importlib.util.spec_from_file_location("checkout_guard", module_path)
            self.assertIsNotNone(spec)
            self.assertIsNotNone(spec.loader)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            executable.unlink()
            with self.assertRaisesRegex(ValueError, "stale or not built"):
                module._guard_path()
            target = executable.with_name("external")
            target.write_bytes(b"external")
            target.chmod(0o755)
            executable.symlink_to(target)
            with self.assertRaisesRegex(ValueError, "untrusted provenance"):
                module._guard_path()

    def test_stale_debug_guard_fails_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            temporary = os.path.realpath(temporary)
            _, module_path = checkout(Path(temporary))
            spec = importlib.util.spec_from_file_location("checkout_guard", module_path)
            self.assertIsNotNone(spec)
            self.assertIsNotNone(spec.loader)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            stale = type("Result", (), {"returncode": 0,
                "stdout": ("0" * 64 + "\n").encode(), "stderr": b""})()
            with patch.object(module.subprocess, "run", return_value=stale):
                with self.assertRaisesRegex(ValueError, "stale or not built"):
                    module._guard_path()

    def test_checkout_guard_can_supply_the_current_fingerprint(self):
        with tempfile.TemporaryDirectory() as temporary:
            temporary = os.path.realpath(temporary)
            expected, module_path = checkout(Path(temporary))
            spec = importlib.util.spec_from_file_location("checkout_guard", module_path)
            self.assertIsNotNone(spec)
            self.assertIsNotNone(spec.loader)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            repository = Path(temporary)
            fingerprint = module._source_fingerprint(repository)
            current = type("Result", (), {"returncode": 0,
                "stdout": (fingerprint + "\n").encode(), "stderr": b""})()
            with patch.object(module.subprocess, "run", return_value=current):
                self.assertEqual(module._guard_path(), expected)

    def test_preflight_uses_exact_bytes_and_empty_environment(self):
        data = b"immutable archive snapshot"
        result = type("Result", (), {"returncode": 0, "stdout": b"", "stderr": b""})()
        with patch.object(PRECHECK, "_guard_path", return_value=Path("/trusted/guard")), \
             patch.object(PRECHECK.subprocess, "run", return_value=result) as run:
            PRECHECK.preflight_archive(data, "semver-capsule")
        arguments, keywords = run.call_args
        self.assertEqual(arguments[0], ["/trusted/guard", "semver-capsule"])
        self.assertIs(keywords["input"], data)
        self.assertEqual(keywords["env"], {})
        self.assertTrue(keywords["close_fds"])

    def test_mutable_archive_input_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "immutable byte snapshot"):
            PRECHECK.preflight_archive(bytearray(b"archive"), "source")

    def test_writable_executable_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            temporary = os.path.realpath(temporary)
            executable, module_path = checkout(Path(temporary))
            spec = importlib.util.spec_from_file_location("checkout_guard", module_path)
            self.assertIsNotNone(spec)
            self.assertIsNotNone(spec.loader)
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            executable.chmod(stat.S_IMODE(executable.stat().st_mode) | stat.S_IWGRP)
            with self.assertRaisesRegex(ValueError, "untrusted provenance"):
                module._guard_path()

    def test_tree_enumeration_rejects_excess_entries_during_iteration(self):
        with tempfile.TemporaryDirectory() as temporary:
            temporary = os.path.realpath(temporary)
            root = Path(temporary)
            source = root / "src"
            source.mkdir()
            for index in range(PRECHECK.SOURCE_ENTRY_LIMIT + 1):
                (source / f"module-{index}.rs").touch()
            descriptor = os.open(root, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
            try:
                with self.assertRaisesRegex(ValueError, "entry count exceeded"):
                    PRECHECK._tree_files(descriptor, "src")
            finally:
                os.close(descriptor)

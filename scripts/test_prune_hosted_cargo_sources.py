"""Unit tests for the fd-relative hosted Cargo source cleanup."""

from __future__ import annotations

import io
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.dont_write_bytecode = True
import prune_hosted_cargo_sources as prune  # noqa: E402


class PruneHostedCargoSourcesTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory(prefix="velnor-prune-test-")
        self.root = Path(os.path.realpath(self.tempdir.name))
        self.runner_temp = self.root / "runner-temp"
        self.cargo_home = self.runner_temp / "velnor" / "cargo"
        self.runner_temp.mkdir()
        self.cargo_home.mkdir(parents=True)
        self.output = io.StringIO()

    def tearDown(self) -> None:
        self.tempdir.cleanup()

    @staticmethod
    def same_mount(_fd: int) -> int:
        return 7

    def run_prune(self, mount_id_reader=None) -> None:
        if mount_id_reader is None:
            mount_id_reader = self.same_mount
        prune.prune_sources(
            str(self.runner_temp),
            str(self.cargo_home),
            mount_id_reader=mount_id_reader,
            output=self.output,
        )

    def test_removes_only_registry_and_git_and_preserves_owned_neighbors(self) -> None:
        registry = self.cargo_home / "registry"
        git = self.cargo_home / "git"
        (registry / "src").mkdir(parents=True)
        git.mkdir()
        (registry / "src" / "crate.rs").write_text("source", encoding="utf-8")
        (git / "checkout").write_text("checkout", encoding="utf-8")

        preserved = [
            self.cargo_home / "bin" / "cargo",
            self.cargo_home / "config.toml",
            self.runner_temp / "velnor" / "target" / "lane" / "target-state",
            self.runner_temp / "velnor" / "cache" / "mbx" / "receipt",
            self.runner_temp / "velnor" / "run" / "tasks" / "report.json",
            self.root / "outside-sentinel",
        ]
        for path in preserved:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("preserve", encoding="utf-8")

        outside_link_target = self.root / "external-tree"
        outside_link_target.mkdir()
        outside_sentinel = outside_link_target / "sentinel"
        outside_sentinel.write_text("external", encoding="utf-8")
        (registry / "external-link").symlink_to(outside_link_target, target_is_directory=True)

        self.run_prune()

        self.assertFalse(registry.exists())
        self.assertFalse(git.exists())
        for path in preserved:
            self.assertTrue(path.is_file(), f"missing preserved file: {path}")
            self.assertEqual(path.read_text(encoding="utf-8"), "preserve")
        self.assertEqual(outside_sentinel.read_text(encoding="utf-8"), "external")
        log = self.output.getvalue()
        for expected in (
            "RUNNER_TEMP phase=before free_bytes=",
            "RUNNER_TEMP phase=after free_bytes=",
            "Cargo source phase=before name=registry allocated_bytes=",
            "Cargo source phase=before name=git allocated_bytes=",
            "Cargo source phase=after name=registry allocated_bytes=0 inodes=0 present=false",
            "Cargo source phase=after name=git allocated_bytes=0 inodes=0 present=false",
        ):
            self.assertIn(expected, log)

    def test_symlinked_ancestor_fails_before_touching_external_source(self) -> None:
        external = self.root / "external"
        (external / "cargo" / "registry").mkdir(parents=True)
        sentinel = external / "cargo" / "registry" / "sentinel"
        sentinel.write_text("keep", encoding="utf-8")
        shutil.rmtree(self.runner_temp / "velnor")
        (self.runner_temp / "velnor").symlink_to(external, target_is_directory=True)

        with self.assertRaises(prune.PruneError):
            self.run_prune()
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep")

    def test_symlinked_absolute_path_ancestor_fails_closed(self) -> None:
        link = self.root / "ancestor-link"
        link.symlink_to(self.root, target_is_directory=True)
        aliased_temp = link / self.runner_temp.name
        aliased_home = aliased_temp / "velnor" / "cargo"
        sentinel = self.cargo_home / "registry" / "source"
        sentinel.parent.mkdir()
        sentinel.write_text("keep", encoding="utf-8")

        with self.assertRaises(prune.PruneError):
            prune.prune_sources(
                str(aliased_temp),
                str(aliased_home),
                mount_id_reader=self.same_mount,
                output=self.output,
            )
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "keep")

    def test_symlinked_source_root_fails_before_removing_the_other_root(self) -> None:
        registry = self.cargo_home / "registry"
        registry.mkdir()
        registry_sentinel = registry / "source"
        registry_sentinel.write_text("keep", encoding="utf-8")
        outside = self.root / "outside-source"
        outside.mkdir()
        outside_sentinel = outside / "external"
        outside_sentinel.write_text("keep", encoding="utf-8")
        (self.cargo_home / "git").symlink_to(outside, target_is_directory=True)

        with self.assertRaises(prune.PruneError):
            self.run_prune()
        self.assertTrue(registry_sentinel.is_file())
        self.assertEqual(outside_sentinel.read_text(encoding="utf-8"), "keep")

    def test_mnt_id_mismatch_in_registry_or_git_prevents_all_mutation(self) -> None:
        sentinels: list[Path] = []
        nested_inodes: dict[int, str] = {}
        for root_name in ("registry", "git"):
            nested_mount = self.cargo_home / root_name / "nested-mount"
            nested_mount.mkdir(parents=True)
            nested_inodes[nested_mount.stat().st_ino] = root_name
            sentinel = nested_mount / "data"
            sentinel.write_text("keep", encoding="utf-8")
            sentinels.append(sentinel)

        for mismatch_root in ("registry", "git"):
            mismatch_inode = next(
                inode for inode, name in nested_inodes.items() if name == mismatch_root
            )

            def injected_mount_id(fd: int) -> int:
                return 99 if os.fstat(fd).st_ino == mismatch_inode else 7

            with self.subTest(mismatch_root=mismatch_root):
                with self.assertRaisesRegex(prune.PruneError, "nested mount"):
                    self.run_prune(injected_mount_id)
                for sentinel in sentinels:
                    self.assertTrue(sentinel.is_file(), f"preflight mutated {sentinel}")

    def test_mount_id_reader_failure_prevents_mutation(self) -> None:
        registry_sentinel = self.cargo_home / "registry" / "source"
        registry_sentinel.parent.mkdir()
        registry_sentinel.write_text("keep", encoding="utf-8")
        git_sentinel = self.cargo_home / "git" / "source"
        git_sentinel.parent.mkdir()
        git_sentinel.write_text("keep", encoding="utf-8")
        registry_inode = registry_sentinel.parent.stat().st_ino

        def failing_mount_id(fd: int) -> int:
            if os.fstat(fd).st_ino == registry_inode:
                raise OSError("injected fdinfo failure")
            return 7

        with self.assertRaisesRegex(OSError, "injected fdinfo failure"):
            self.run_prune(failing_mount_id)
        self.assertTrue(registry_sentinel.is_file())
        self.assertTrue(git_sentinel.is_file())

    def test_mount_added_after_preflight_is_rejected_before_entering_it(self) -> None:
        nested = self.cargo_home / "registry" / "nested-mount"
        nested.mkdir(parents=True)
        sentinel = nested / "data"
        sentinel.write_text("keep", encoding="utf-8")
        nested_inode = nested.stat().st_ino
        reads = 0

        def changes_after_preflight(fd: int) -> int:
            nonlocal reads
            if os.fstat(fd).st_ino == nested_inode:
                reads += 1
                if reads == 3:
                    return 99
            return 7

        with self.assertRaisesRegex(prune.PruneError, "nested mount"):
            self.run_prune(changes_after_preflight)
        self.assertTrue(sentinel.is_file())

    def test_mounted_regular_file_fails_before_any_mutation_on_linux(self) -> None:
        if not hasattr(os, "O_PATH"):
            self.skipTest("O_PATH file mount checks are Linux-specific")
        registry_file = self.cargo_home / "registry" / "source"
        registry_file.parent.mkdir()
        registry_file.write_text("keep", encoding="utf-8")
        git_file = self.cargo_home / "git" / "source"
        git_file.parent.mkdir()
        git_file.write_text("keep", encoding="utf-8")
        mounted_inode = registry_file.stat().st_ino

        def mounted_file_id(fd: int) -> int:
            return 99 if os.fstat(fd).st_ino == mounted_inode else 7

        with self.assertRaisesRegex(prune.PruneError, "mounted entry"):
            self.run_prune(mounted_file_id)
        self.assertTrue(registry_file.is_file())
        self.assertTrue(git_file.is_file())

    def test_noncanonical_or_wrong_cargo_home_fails_closed(self) -> None:
        for bad_temp, bad_home in (
            (str(self.runner_temp / ".."), str(self.cargo_home)),
            (str(self.runner_temp), str(self.cargo_home / "..")),
            (f"/{self.runner_temp}", f"/{self.cargo_home}"),
            ("", ""),
            ("relative", "relative/velnor/cargo"),
        ):
            with self.subTest(runner_temp=bad_temp, cargo_home=bad_home):
                with self.assertRaises(prune.PruneError):
                    prune.prune_sources(
                        bad_temp,
                        bad_home,
                        mount_id_reader=self.same_mount,
                        output=self.output,
                    )


if __name__ == "__main__":
    unittest.main()

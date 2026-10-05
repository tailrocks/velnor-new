"""Input verifier regressions; synthetic data makes no execution authority claim."""

import argparse
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("binding", Path(__file__).with_name("bind_inputs.py"))
BINDING = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BINDING)


class InputClosureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / "source"
        self.source.mkdir()
        self.data = b"reviewed source\n"
        (self.source / "lib.rs").write_bytes(self.data)
        (self.source / ".cargo-ok").write_bytes(b"")
        self.archive = self.root / "tiny.crate"
        with tarfile.open(self.archive, "w:gz") as stream:
            member = tarfile.TarInfo("tiny-1.0.0/lib.rs")
            member.size = len(self.data)
            stream.addfile(member, io.BytesIO(self.data))
        records = [dict(path="lib.rs", size=len(self.data), sha256=BINDING.sha(self.data))]
        self.spec = dict(name="tiny", version="1.0.0", files=records,
                         archive_sha256=BINDING.sha(self.archive.read_bytes()),
                         inventory_sha256=BINDING.inventory_sha(records),
                         local_extraction_marker=dict(path=".cargo-ok", size=0,
                                                      sha256=BINDING.sha(b"")))

    def test_exact_source_inventory_accepts(self):
        BINDING.verify_registry(self.spec, self.archive, self.source)

    def test_extra_resource_rejects(self):
        (self.source / "extra.rs").write_bytes(b"unreviewed")
        with self.assertRaisesRegex(ValueError, "unexpected registry source input"):
            BINDING.verify_registry(self.spec, self.archive, self.source)

    def test_changed_source_rejects(self):
        (self.source / "lib.rs").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "registry extraction differs"):
            BINDING.verify_registry(self.spec, self.archive, self.source)

    def test_changed_archive_rejects(self):
        self.archive.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "archive checksum differs"):
            BINDING.verify_registry(self.spec, self.archive, self.source)

    def test_extra_symlink_rejects(self):
        (self.source / "extra.rs").symlink_to(self.source / "lib.rs")
        with self.assertRaisesRegex(ValueError, "source symlink forbidden"):
            BINDING.verify_registry(self.spec, self.archive, self.source)

    def test_changed_manifest_rejects(self):
        raw = json.dumps(dict(scope="exact_synchronous_fixture_v1")).encode()
        (self.root / "manifest.json").write_bytes(raw)
        args = argparse.Namespace(expected_manifest_sha256=BINDING.sha(b"approved bytes"))
        with patch.object(BINDING, "ROOT", self.root):
            with self.assertRaisesRegex(ValueError, "reviewed manifest digest differs"):
                BINDING.verify(args)

    def repository_layout(self):
        repository = self.root / "repository"
        harness = repository / "scripts/qualification/mbx-synchronous"
        harness.mkdir(parents=True)
        return repository, harness

    def fixture_manifest(self, root="../../../crates/velnor-actions-mise/tests/fixtures/registry-fixture"):
        return dict(fixture=dict(root=root))

    def test_default_fixture_root_symlink_rejects_before_resolution(self):
        repository, harness = self.repository_layout()
        fixture = repository / "crates/velnor-actions-mise/tests/fixtures/registry-fixture"
        fixture.parent.mkdir(parents=True)
        fixture.symlink_to(self.source, target_is_directory=True)
        with patch.object(BINDING, "ROOT", harness):
            with self.assertRaisesRegex(ValueError, "symlink component"):
                BINDING.fixture_root(argparse.Namespace(fixture_root=None),
                                     self.fixture_manifest())

    def test_default_fixture_ancestor_symlink_rejects_before_resolution(self):
        repository, harness = self.repository_layout()
        external = self.root / "external-crates"
        external.mkdir()
        (repository / "crates").symlink_to(external, target_is_directory=True)
        with patch.object(BINDING, "ROOT", harness):
            with self.assertRaisesRegex(ValueError, "symlink component"):
                BINDING.fixture_root(argparse.Namespace(fixture_root=None),
                                     self.fixture_manifest())

    def test_manifest_symlink_before_dotdot_rejects(self):
        repository, harness = self.repository_layout()
        crates = repository / "crates"
        crates.mkdir()
        (crates / "symlink").symlink_to(self.root, target_is_directory=True)
        fixture = crates / "velnor-actions-mise/tests/fixtures/registry-fixture"
        fixture.mkdir(parents=True)
        manifest = self.fixture_manifest(
            "../../../crates/symlink/../velnor-actions-mise/tests/fixtures/registry-fixture")
        with patch.object(BINDING, "ROOT", harness):
            with self.assertRaisesRegex(ValueError, "symlink component"):
                BINDING.fixture_source(manifest)

    def test_manifest_fixture_root_dotdot_normalizes_lexically(self):
        repository, harness = self.repository_layout()
        fixture = repository / "crates/velnor-actions-mise/tests/fixtures/registry-fixture"
        fixture.mkdir(parents=True)
        manifest = self.fixture_manifest(
            "../../../crates/unused/../velnor-actions-mise/tests/fixtures/registry-fixture")
        with patch.object(BINDING, "ROOT", harness):
            self.assertEqual(BINDING.fixture_source(manifest), fixture)

    def test_manifest_fixture_root_cannot_escape_repository(self):
        _, harness = self.repository_layout()
        with patch.object(BINDING, "ROOT", harness):
            with self.assertRaisesRegex(ValueError, "escapes the repository"):
                BINDING.fixture_source(self.fixture_manifest("../../../../outside"))

    def copied_fixture(self):
        root = Path(__file__).resolve().parent
        manifest = json.loads((root / "manifest.json").read_bytes())
        copy = self.root / "executed"
        BINDING.copy_fixture(BINDING.fixture_source(manifest), copy)
        args = argparse.Namespace(fixture_root=copy,
                                  expected_manifest_sha256=BINDING.sha(
                                      (root / "manifest.json").read_bytes()),
                                  registry_archive=self.archive, registry_source=self.source)
        return args

    def test_copied_fixture_materializes_pinned_lock_bytes(self):
        args = self.copied_fixture()
        manifest = json.loads((Path(__file__).resolve().parent / "manifest.json").read_bytes())
        expected = (BINDING.fixture_source(manifest) / "Cargo.lock.fixture").read_bytes()
        self.assertEqual((args.fixture_root / "Cargo.lock").read_bytes(), expected)
        self.assertFalse((args.fixture_root / "Cargo.lock.fixture").exists())

    def test_valid_executed_copy_accepts(self):
        args = self.copied_fixture()
        with patch.object(BINDING, "verify_registry"):
            manifest, _ = BINDING.verify(args)
        self.assertEqual(BINDING.fixture_root(args, manifest), args.fixture_root)

    def test_changed_executed_copy_rejects(self):
        args = self.copied_fixture()
        (args.fixture_root / "src/lib.rs").write_bytes(b"changed consumed source")
        with self.assertRaisesRegex(ValueError, "unexpected or changed fixture input"):
            BINDING.verify(args)

    def test_symlink_executed_copy_rejects(self):
        args = self.copied_fixture()
        alias = self.root / "alias"
        alias.symlink_to(args.fixture_root, target_is_directory=True)
        args.fixture_root = alias
        with self.assertRaisesRegex(ValueError, "symlink component"):
            BINDING.verify(args)

    def test_symlink_ancestor_rejects(self):
        args = self.copied_fixture()
        alias = self.root / "ancestor"
        alias.symlink_to(self.root, target_is_directory=True)
        args.fixture_root = alias / args.fixture_root.name
        with self.assertRaisesRegex(ValueError, "symlink component"):
            BINDING.verify(args)


if __name__ == "__main__":
    unittest.main()

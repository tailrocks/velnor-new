"""Bounded relocation proof; fixture projection grants no production warmth."""
import sys
import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"))
import cache_receipt as receipt
import cache_receipt_materialize as materialize
from cache_receipt_common import ColdReceipt
from cache_receipt_manifest import canonical, inventory_exact_roots
from cache_receipt_materialize_transaction import MaterializationStop
from cache_receipt_test import layout, policy, replace


class _FixtureProjection:
    def __init__(self, compiled):
        self.compiled = compiled

    def require(self, compiled):
        if compiled is not self.compiled:
            raise ColdReceipt("fixture_projection")

    def inventory_live(self, root, compiled):
        self.require(compiled)
        return inventory_exact_roots(root, compiled.allowed_roots, compiled.optional_roots)


@patch("source_archive_inventory_leaf.metadata_records", return_value=[])
@patch("cache_receipt_materialize.metadata_records", return_value=[])
class MaterializeTests(unittest.TestCase):
    def prepare(self, base, linked=False, optional=False):
        producer, stage, runner = (base / name for name in ("producer", "quarantine", "runner"))
        roots = ("mise", "rustup", "cargo/.crates.toml") if optional else ("mise", "cargo/bin")
        missing = ("cargo/.crates.toml",) if optional else ()
        binary_root = roots[1]
        (producer / binary_root).mkdir(parents=True)
        (producer / binary_root / "tool").write_bytes(b"authenticated executable")
        (producer / binary_root / "tool").chmod(0o755)
        if linked:
            (producer / "mise").symlink_to(binary_root)
        else:
            (producer / "mise").mkdir()
            (producer / "mise/tool").symlink_to("../" + binary_root + "/tool")
        data = inventory_exact_roots(producer, roots, missing)
        (stage / "roots").mkdir(parents=True)
        stage.chmod(0o700)
        runner.mkdir(mode=0o700)
        shutil.copytree(producer / binary_root, stage / "roots/1")
        if linked:
            (stage / "roots/0").symlink_to("1")
        else:
            (stage / "roots/0").mkdir()
            (stage / "roots/0/tool").symlink_to("../1/tool")
        evidence = stage / "roots" / str(len(roots))
        evidence.mkdir()
        (evidence / "manifest.json").write_bytes(data)
        predicate = canonical({"fixture": True})
        (evidence / "predicate.json").write_bytes(predicate)
        (evidence / "bundle.sigstore.json").write_bytes(b"bundle")
        compiled = replace(policy(), allowed_roots=roots, optional_roots=missing,
                           transport_layout=layout(roots, missing))
        with patch.object(receipt, "_verify"), patch.object(receipt, "_admit",
                return_value=receipt._ReceiptObservation("unused", 1, 1, predicate)):
            grant = receipt.verify_payload(stage, None, compiled)
        return runner, stage, grant, compiled

    def capture(self, runner, compiled):
        projection = _FixtureProjection(compiled)
        changes = {"_COMPILED_SOURCE_BINDING": compiled, "_COMPILED_PROJECTION": projection,
                   "_COMPILED_PROJECTION_TYPE": _FixtureProjection}
        return patch.multiple(materialize, **changes), patch.dict(os.environ,
            RUNNER_TEMP=str(runner), VELNOR_CACHE_PAYLOAD_ROOT=str(runner / "velnor"))

    def test_relocated_descendant_and_root_links_close(self, *_metadata):
        for linked in (False, True):
            with self.subTest(root_link=linked), tempfile.TemporaryDirectory() as temporary:
                runner, _, grant, compiled = self.prepare(Path(temporary).resolve(), linked)
                binding, environment = self.capture(runner, compiled)
                with binding, environment:
                    namespace = materialize._capture_source_namespace()
                    try:
                        self.assertEqual(materialize._materialize_verified(grant, namespace),
                                         grant.manifest_sha256)
                        root = runner / "velnor"
                        logical_link = root / "mise" if linked else root / "mise/tool"
                        self.assertEqual(os.readlink(logical_link), "cargo/bin" if linked
                                         else "../cargo/bin/tool")
                        executable = logical_link / "tool" if linked else logical_link
                        self.assertEqual(executable.read_bytes(), b"authenticated executable")
                        self.assertEqual((root / "cargo").stat().st_mode & 0o777, 0o700)
                    finally:
                        namespace.close()

    def test_forged_grants_and_destination_authority_reject(self, *_metadata):
        with self.assertRaisesRegex(ColdReceipt, "materialize_authority"):
            materialize._materialize_verified(object(), object())
        with self.assertRaisesRegex(ColdReceipt, "materialize_namespace_authority"):
            materialize._CanonicalNamespace(object(), None, "/outside", None, None)
        with self.assertRaisesRegex(ColdReceipt, "materialize_source_unqualified"):
            materialize._capture_source_namespace()

    def test_binding_alias_and_symlink_ancestors_reject(self, *_metadata):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            runner, _, _, compiled = self.prepare(base)
            binding, environment = self.capture(runner, compiled)
            with binding, environment:
                for path in (str(runner) + "/../runner", str(runner) + "/."):
                    with patch.dict(os.environ, RUNNER_TEMP=path), self.assertRaises(ColdReceipt):
                        materialize._capture_source_namespace()
                with patch.dict(os.environ, VELNOR_CACHE_PAYLOAD_ROOT=str(base / "outside")):
                    with self.assertRaisesRegex(ColdReceipt, "materialize_namespace_binding"):
                        materialize._capture_source_namespace()
                (runner / "velnor").symlink_to(base)
                with self.assertRaises(OSError):
                    materialize._capture_source_namespace()

    def test_fifo_existing_root_and_linked_structural_parent_reject(self, *_metadata):
        for kind in ("fifo", "existing", "parent_link"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temporary:
                base = Path(temporary).resolve()
                runner, _, grant, compiled = self.prepare(base)
                binding, environment = self.capture(runner, compiled)
                with binding, environment:
                    namespace = materialize._capture_source_namespace()
                    try:
                        destination = runner / "velnor"
                        if kind == "fifo":
                            os.mkfifo(destination / "mise")
                        elif kind == "existing":
                            (destination / "mise").mkdir()
                        else:
                            (destination / "cargo").symlink_to(base)
                        with self.assertRaises((ColdReceipt, OSError)):
                            materialize._materialize_verified(grant, namespace)
                        self.assertFalse((base / "bin").exists())
                    finally:
                        namespace.close()

    def test_mutating_stage_and_unknown_extra_reject_before_writes(self, *_metadata):
        for kind in ("changed", "extra"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as temporary:
                runner, stage, grant, compiled = self.prepare(Path(temporary).resolve())
                binding, environment = self.capture(runner, compiled)
                with binding, environment:
                    namespace = materialize._capture_source_namespace()
                    try:
                        if kind == "changed":
                            (stage / "roots/1/tool").write_bytes(b"changed")
                        else:
                            (stage / "roots/3").mkdir()
                        with self.assertRaises(ColdReceipt):
                            materialize._materialize_verified(grant, namespace)
                        self.assertEqual(list((runner / "velnor").iterdir()), [])
                    finally:
                        namespace.close()

    def test_stage_mutation_during_copy_never_grants_materialization(self, *_metadata):
        with tempfile.TemporaryDirectory() as temporary:
            runner, stage, grant, compiled = self.prepare(Path(temporary).resolve())
            binding, environment = self.capture(runner, compiled)
            original = materialize._copy_file
            def mutate_after_copy(*args):
                original(*args)
                (stage / "roots/1/tool").write_bytes(b"mutated after copy")
            with binding, environment:
                namespace = materialize._capture_source_namespace()
                try:
                    with patch.object(materialize, "_copy_file", side_effect=mutate_after_copy):
                        with self.assertRaises(ColdReceipt):
                            materialize._materialize_verified(grant, namespace)
                finally:
                    namespace.close()
                self.assertFalse((runner / "velnor/mise").exists())
                self.assertFalse((runner / "velnor/cargo/bin").exists())
                self.assertFalse(any(path.name.startswith(".materialize-")
                                     for path in (runner / "velnor").iterdir()))

    def test_projection_type_and_captured_namespace_replacement_reject(self, *_metadata):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            runner, _, grant, compiled = self.prepare(base)
            binding, environment = self.capture(runner, compiled)
            with binding, environment:
                with patch.object(materialize, "_COMPILED_PROJECTION_TYPE", object):
                    with self.assertRaisesRegex(ColdReceipt, "materialize_projection_authority"):
                        materialize._capture_source_namespace()
                namespace = materialize._capture_source_namespace()
                try:
                    (runner / "velnor").rename(runner / "previous")
                    (runner / "velnor").mkdir(mode=0o700)
                    with self.assertRaisesRegex(MaterializationStop, "materialize_namespace_unsafe"):
                        materialize._materialize_verified(grant, namespace)
                    self.assertEqual(list((runner / "velnor").iterdir()), [])
                finally:
                    namespace.close()

    def test_copy_mode_commit_and_final_inventory_failures_roll_back(self, *_metadata):
        from cache_receipt_materialize_transaction import _Assembly
        for failure in ("copy", "mode", "commit", "live_inventory"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temporary:
                runner, _, grant, compiled = self.prepare(Path(temporary).resolve())
                binding, environment = self.capture(runner, compiled)
                with binding, environment:
                    namespace = materialize._capture_source_namespace()
                    original_commit = _Assembly.commit
                    original_modes = materialize._finish_directories
                    original_inventory = namespace._projection.inventory_live
                    count = 0
                    def partial_commit(assembly, verified):
                        original_commit(assembly, verified)
                        raise OSError("fixture commit failure")
                    def final_mode_failure(*args):
                        original_modes(*args)
                        raise OSError("fixture final mode failure")
                    def changed_live_inventory(*args):
                        nonlocal count
                        count += 1
                        return original_inventory(*args) if count == 1 else b"mismatched"
                    if failure == "copy":
                        failing = patch.object(materialize.os, "write", side_effect=OSError("copy"))
                    elif failure == "mode":
                        failing = patch.object(materialize, "_finish_directories",
                                               side_effect=final_mode_failure)
                    elif failure == "commit":
                        failing = patch.object(_Assembly, "commit", partial_commit)
                    else:
                        failing = patch.object(namespace._projection, "inventory_live",
                                               side_effect=changed_live_inventory)
                    try:
                        with failing, self.assertRaises((ColdReceipt, OSError)):
                            materialize._materialize_verified(grant, namespace)
                        self.assertFalse((runner / "velnor/mise").exists())
                        self.assertFalse((runner / "velnor/cargo/bin").exists())
                        self.assertFalse(any(path.name.startswith(".materialize-")
                                             for path in (runner / "velnor").iterdir()))
                    finally:
                        namespace.close()

    def test_optional_root_without_structural_parent_stays_missing(self, *_metadata):
        with tempfile.TemporaryDirectory() as temporary:
            runner, _, grant, compiled = self.prepare(Path(temporary).resolve(), optional=True)
            binding, environment = self.capture(runner, compiled)
            with binding, environment:
                namespace = materialize._capture_source_namespace()
                try:
                    materialize._materialize_verified(grant, namespace)
                    self.assertFalse((runner / "velnor/cargo").exists())
                finally:
                    namespace.close()

    def test_rollback_replacement_is_terminal_and_not_cold_fallback(self, *_metadata):
        from cache_receipt_materialize_transaction import _Assembly
        with tempfile.TemporaryDirectory() as temporary:
            runner, _, grant, compiled = self.prepare(Path(temporary).resolve())
            binding, environment = self.capture(runner, compiled)
            original = _Assembly.commit
            def replace_after_commit(assembly, verified):
                original(assembly, verified)
                root = runner / "velnor"
                (root / "mise").rename(root / "retained-original")
                (root / "mise").mkdir()
                (root / "mise/untrusted").write_bytes(b"must not execute")
                raise OSError("fixture postcommit failure")
            with binding, environment:
                namespace = materialize._capture_source_namespace()
                try:
                    with patch.object(_Assembly, "commit", replace_after_commit):
                        with self.assertRaisesRegex(MaterializationStop, "materialize_rollback_failed"):
                            try:
                                materialize._materialize_verified(grant, namespace)
                            except ColdReceipt:
                                self.fail("terminal failure entered cold fallback")
                    self.assertEqual((runner / "velnor/mise/untrusted").read_bytes(),
                                     b"must not execute")
                finally:
                    namespace.close()


if __name__ == "__main__":
    unittest.main()

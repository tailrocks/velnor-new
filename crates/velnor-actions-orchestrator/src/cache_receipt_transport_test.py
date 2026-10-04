"""Sealed numbered transport excludes circular evidence and rejects aliases."""
import sys
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"))
import cache_receipt as receipt
from cache_receipt_common import ColdReceipt, MAX_EVIDENCE
from cache_receipt_manifest import canonical
from cache_receipt_test import layout, policy, replace
from cache_receipt_virtual import (
    _AuthenticatedManifest, _WITNESS_AUTHORITY, inventory_quarantine, inventory_receipt_quarantine,
    read_receipt_evidence,
)


def observe(root, compiled=None):
    compiled = policy() if compiled is None else compiled
    evidence = read_receipt_evidence(root, compiled)
    witness = _AuthenticatedManifest(_WITNESS_AUTHORITY, evidence["manifest.json"], compiled)
    return inventory_receipt_quarantine(root, compiled, witness)


@patch("source_archive_inventory_leaf.metadata_records", return_value=[])
class TransportTests(unittest.TestCase):
    def prepare(self, root):
        (root / "roots" / "0").mkdir(parents=True)
        (root / "roots" / "0" / "binary").write_bytes(b"payload")
        data = inventory_quarantine(root, ("tools",))
        evidence = root / "roots" / "1"
        evidence.mkdir()
        (evidence / "manifest.json").write_bytes(data)
        (evidence / "predicate.json").write_bytes(canonical({"schema": 1}))
        (evidence / "bundle.sigstore.json").write_bytes(b"signed bundle")
        return data, evidence

    def test_evidence_excluded_and_only_compiled_index_selects_bundle(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            data, evidence = self.prepare(root)
            (root / "archive-admission.json").write_text('{"evidence_index":0}')
            (root / "bundle.sigstore.json").write_bytes(b"forged outside bundle")
            actual, files = observe(root)
            self.assertEqual(actual, data)
            self.assertNotIn(b"manifest.json", data)
            with patch.object(receipt, "_verify") as verify, patch.object(receipt, "_admit") as admit:
                admit.return_value.predicate_bytes = canonical({"schema": 1})
                receipt.verify_payload(root, None, policy())
                self.assertEqual(verify.call_args.args[1], b"signed bundle")
                admit.assert_called_once()
            (evidence / "bundle.sigstore.json").write_bytes(b"different bundle")
            changed, changed_files = observe(root)
            self.assertEqual(data, changed)
            self.assertNotEqual(files, changed_files)

    def test_sealed_layout_frozen_and_closed(self, _metadata):
        source = layout()
        original = replace(policy(), transport_layout=source)
        source["payload_indices"] = (7,)
        original.require_qualified()
        projected = original.source_record()
        projected["transport_layout"]["evidence_index"] = 0
        original.require_qualified()
        mutations = {"schema": True, "payload_indices": (True,), "evidence_index": 0,
                     "evidence_root": "cache-receipts/../outside", "sdk_paths": ("/outside",),
                     "payload_roots": ("other",), "optional_roots": ("tools",),
                     "evidence_files": ("manifest.json", "bundle.sigstore.json")}
        for name, value in mutations.items():
            with self.subTest(name=name), self.assertRaises(ColdReceipt):
                replace(policy(), transport_layout=dict(layout(), **{name: value})).require_qualified()
        with self.assertRaisesRegex(ColdReceipt, "receipt_transport_layout"):
            replace(policy(), transport_layout=dict(layout(), arbitrary="authority"))
        with self.assertRaisesRegex(ColdReceipt, "receipt_policy_authority"):
            read_receipt_evidence("/managed", object())

    def test_unknown_indices_missing_files_and_extra_files_reject(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            _, evidence = self.prepare(root)
            for name in ("2", "01", "-1"):
                extra = root / "roots" / name
                extra.mkdir()
                with self.subTest(name=name), self.assertRaisesRegex(ColdReceipt, "quarantine_unknown_index"):
                    observe(root)
                extra.rmdir()
            for name in policy().transport_layout.evidence_files:
                file = evidence / name
                data = file.read_bytes()
                file.unlink()
                with self.subTest(name=name), self.assertRaisesRegex(ColdReceipt, "quarantine_evidence_files"):
                    observe(root)
                file.write_bytes(data)
            (evidence / "extra").mkdir()
            with self.assertRaisesRegex(ColdReceipt, "quarantine_evidence_files"):
                observe(root)
            (evidence / "extra").rmdir()
            (evidence / "manifest.json").write_bytes(b"forged manifest")
            with self.assertRaises(ColdReceipt):
                observe(root)

    def test_optional_payload_absence_keeps_evidence_final_index(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            data, evidence = self.prepare(root)
            roots, optional = ("tools", "cargo/.crates.toml"), ("cargo/.crates.toml",)
            evidence.rename(root / "roots" / "2")
            compiled = replace(policy(), allowed_roots=roots, optional_roots=optional,
                               transport_layout=layout(roots, optional))
            from cache_receipt_manifest import canonical
            from cache_receipt_common import strict_json
            record = strict_json(data)
            record["entries"].append(dict(path=optional[0], kind="missing", mode=0,
                                          sha256=None, target=None))
            record["entries"].sort(key=lambda entry: entry["path"])
            expected = canonical(record)
            (root / "roots" / "2" / "manifest.json").write_bytes(expected)
            payload, _ = observe(root, compiled)
            self.assertEqual(payload, expected)
            (root / "roots" / "0" / "binary").unlink()
            (root / "roots" / "0").rmdir()
            with self.assertRaisesRegex(ColdReceipt, "quarantine_required_root_missing"):
                observe(root, compiled)

    def test_evidence_fifo_directory_symlink_hardlink_and_size_reject(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            _, evidence = self.prepare(root)
            file = evidence / "bundle.sigstore.json"
            outside = root / "outside"
            outside.write_bytes(b"outside")
            file.unlink()
            for kind in ("fifo", "directory", "symlink", "hardlink", "oversize"):
                if kind == "fifo":
                    os.mkfifo(file)
                elif kind == "directory":
                    file.mkdir()
                elif kind == "symlink":
                    file.symlink_to(outside)
                elif kind == "hardlink":
                    os.link(outside, file)
                else:
                    with file.open("wb") as stream:
                        stream.truncate(MAX_EVIDENCE + 1)
                with self.subTest(kind=kind), self.assertRaises(ColdReceipt):
                    observe(root)
                file.rmdir() if kind == "directory" else file.unlink()

    def test_missing_and_linked_evidence_root_and_lexical_alias_reject(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            _, evidence = self.prepare(root)
            moved = root / "outside"
            evidence.rename(moved)
            with self.assertRaises(ColdReceipt):
                observe(root)
            evidence.symlink_to(moved)
            with self.assertRaises(ColdReceipt):
                observe(root)
            with self.assertRaisesRegex(ColdReceipt, "payload_root_alias"):
                read_receipt_evidence(str(root) + "/./", policy())

    def test_payload_and_evidence_changes_during_verification_reject(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            _, evidence = self.prepare(root)
            for file in (root / "roots" / "0" / "binary", evidence / "predicate.json"):
                before = file.read_bytes()
                def mutate(*_args):
                    file.write_bytes(b"changed")
                with patch.object(receipt, "_verify", side_effect=mutate), patch.object(receipt, "_admit"):
                    with self.subTest(file=file.name), self.assertRaises(ColdReceipt):
                        receipt.verify_payload(root, None, policy())
                file.write_bytes(before)


if __name__ == "__main__":
    unittest.main()

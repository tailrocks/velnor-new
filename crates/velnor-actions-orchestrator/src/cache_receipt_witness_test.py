"""Authenticated targets preserve raw aliases after exact numbered projection."""
import sys
import os
import posixpath
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"))
import cache_receipt as receipt
from cache_receipt_common import ColdReceipt, strict_json
from cache_receipt_manifest import canonical, inventory_exact_roots
from cache_receipt_test import evidence, layout, policy, replace
from cache_receipt_virtual import (
    _AuthenticatedManifest, _WITNESS_AUTHORITY, inventory_receipt_quarantine,
)


def fixture(base, target="./binary", roots=("tools",)):
    producer, quarantine = base / "producer", base / "quarantine"
    for root in roots:
        (producer / root).mkdir(parents=True)
    (producer / "tools" / "binary").write_bytes(b"payload")
    source = producer / roots[-1] / "link"
    source.symlink_to(target)
    data = inventory_exact_roots(producer, roots)
    for index, root in enumerate(roots):
        shutil.copytree(producer / root, quarantine / "roots" / str(index), symlinks=True)
    link_path = posixpath.join(roots[-1], "link")
    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(link_path), target))
    target_index = next(index for index, root in enumerate(roots)
                        if resolved == root or resolved.startswith(root + "/"))
    mapped = "roots/" + str(target_index) + resolved[len(roots[target_index]):]
    numbered_link = quarantine / "roots" / str(len(roots) - 1) / "link"
    numbered_link.unlink()
    numbered_link.symlink_to(posixpath.relpath(mapped, "roots/" + str(len(roots) - 1)))
    evidence_root = quarantine / "roots" / str(len(roots))
    evidence_root.mkdir()
    predicate = evidence(data)[0]["verificationResult"]["statement"]["predicate"]
    for name, content in (("manifest.json", data), ("predicate.json", canonical(predicate)),
                          ("bundle.sigstore.json", b"signed bundle")):
        (evidence_root / name).write_bytes(content)
    compiled = replace(policy(), allowed_roots=roots, transport_layout=layout(roots))
    return quarantine, compiled, data, numbered_link


def verify_fixture(quarantine, compiled):
    def verify(_gh, _bundle, data, _policy):
        return receipt._GhVerified(evidence(data), receipt._VERIFIED)
    with patch.object(receipt, "_verify", side_effect=verify), patch.object(receipt, "_attempt"):
        return receipt.verify_payload(quarantine, None, compiled)


@patch("source_archive_inventory_leaf.metadata_records", return_value=[])
class WitnessTests(unittest.TestCase):
    def test_raw_aliases_have_distinct_signed_manifests_and_same_numbered_target(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            records = []
            for index, target in enumerate(("binary", "./binary", "../tools/binary")):
                quarantine, compiled, data, link = fixture(base / str(index), target)
                self.assertEqual(os.readlink(link), "binary")
                grant = verify_fixture(quarantine, compiled)
                grant.require_current()
                self.assertEqual(grant.manifest_bytes, data)
                entries = strict_json(grant.manifest_bytes)["entries"]
                signed = next(entry for entry in entries if entry["kind"] == "symlink")
                self.assertEqual(signed["target"], target)
                records.append(data)
            self.assertEqual(len(set(records)), 3)

    def test_multiroot_relative_mapping_and_fresh_relocated_quarantine(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary).resolve()
            roots = ("tools", "cargo/bin")
            quarantine, compiled, data, link = fixture(base, "../../tools/binary", roots)
            self.assertEqual(os.readlink(link), "../0/binary")
            grant = verify_fixture(quarantine, compiled)
            grant.require_current()
            relocated = base / "fresh-runner" / "quarantine"
            shutil.copytree(quarantine, relocated, symlinks=True)
            moved = verify_fixture(relocated, compiled)
            moved.require_current()
            self.assertEqual(moved.manifest_bytes, data)
            self.assertNotEqual(moved.quarantine, grant.quarantine)

    def test_crypto_and_closed_identity_precede_witness_and_inventory(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            quarantine, compiled, _, _ = fixture(Path(temporary).resolve())
            with patch.object(receipt, "_verify", side_effect=ColdReceipt("bad signature")), \
                    patch.object(receipt, "_AuthenticatedManifest") as mint, \
                    patch.object(receipt, "inventory_receipt_quarantine") as observe:
                with self.assertRaisesRegex(ColdReceipt, "bad signature"):
                    receipt.verify_payload(quarantine, None, compiled)
                mint.assert_not_called()
                observe.assert_not_called()
            def shadow(_gh, _bundle, data, _policy):
                value = evidence(data)
                value[0]["verificationResult"]["signature"]["certificate"]["buildSignerURI"] = "shadow"
                return receipt._GhVerified(value, receipt._VERIFIED)
            with patch.object(receipt, "_verify", side_effect=shadow), \
                    patch.object(receipt, "_AuthenticatedManifest") as mint:
                with self.assertRaisesRegex(ColdReceipt, "certificate_identity"):
                    receipt.verify_payload(quarantine, None, compiled)
                mint.assert_not_called()

    def test_observed_alias_absolute_and_wrong_root_targets_reject(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            quarantine, compiled, _, link = fixture(Path(temporary).resolve())
            for target in ("./binary", "/outside", "../1/manifest.json", "../../outside"):
                link.unlink()
                link.symlink_to(target)
                with self.subTest(target=target), self.assertRaisesRegex(ColdReceipt, "quarantine_symlink_transform"):
                    verify_fixture(quarantine, compiled)

    def test_authenticated_original_absolute_external_chain_and_duplicate_reject(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            quarantine, compiled, data, _ = fixture(Path(temporary).resolve())
            evidence_manifest = quarantine / "roots" / "1" / "manifest.json"
            for target in ("/publisher/tools/binary", "../../outside", "link", "C:binary"):
                record = strict_json(data)
                signed = next(entry for entry in record["entries"] if entry["kind"] == "symlink")
                signed["target"] = target
                mutated = canonical(record)
                evidence_manifest.write_bytes(mutated)
                (evidence_manifest.parent / "predicate.json").write_bytes(
                    canonical(evidence(mutated)[0]["verificationResult"]["statement"]["predicate"]))
                with self.subTest(target=target), self.assertRaises(ColdReceipt):
                    verify_fixture(quarantine, compiled)
            record = strict_json(data)
            record["entries"].append(record["entries"][-1])
            mutated = canonical(record)
            evidence_manifest.write_bytes(mutated)
            (evidence_manifest.parent / "predicate.json").write_bytes(
                canonical(evidence(mutated)[0]["verificationResult"]["statement"]["predicate"]))
            with self.assertRaisesRegex(ColdReceipt, "receipt_manifest_entry"):
                verify_fixture(quarantine, compiled)

    def test_witness_grant_authority_immutability_and_current_payload(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            quarantine, compiled, data, link = fixture(Path(temporary).resolve())
            with self.assertRaisesRegex(ColdReceipt, "receipt_manifest_authority"):
                _AuthenticatedManifest(None, data, compiled)
            with self.assertRaisesRegex(ColdReceipt, "quarantine_payload_authority"):
                receipt.VerifiedQuarantinePayload(None, quarantine, compiled, None, {}, None)
            witness = _AuthenticatedManifest(_WITNESS_AUTHORITY, data, compiled)
            with self.assertRaisesRegex(ColdReceipt, "receipt_manifest_authority"):
                inventory_receipt_quarantine(quarantine, policy(), witness)
            with self.assertRaisesRegex(ColdReceipt, "receipt_manifest_immutable"):
                witness._data = b"forged"
            grant = verify_fixture(quarantine, compiled)
            with self.assertRaisesRegex(ColdReceipt, "quarantine_payload_immutable"):
                grant._quarantine = "/outside"
            forged = object.__new__(receipt.VerifiedQuarantinePayload)
            with self.assertRaisesRegex(ColdReceipt, "quarantine_payload_authority"):
                forged.require_current()
            link.unlink()
            link.symlink_to("./binary")
            with self.assertRaisesRegex(ColdReceipt, "quarantine_symlink_transform"):
                grant.require_current()

    def test_redundant_predicate_exact_signed_canonical_and_duplicate_free(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            quarantine, compiled, data, _ = fixture(Path(temporary).resolve())
            predicate_file = quarantine / "roots" / "1" / "predicate.json"
            producer_bytes = canonical(evidence(data)[0]["verificationResult"]["statement"]["predicate"])
            self.assertEqual(predicate_file.read_bytes(), producer_bytes)
            verify_fixture(quarantine, compiled).require_current()
            changed = strict_json(producer_bytes)
            changed["cache_key"] = "attacker-key"
            for raw in (canonical(changed), b'{"schema":1,"schema":1}', b'{"schema":NaN}',
                        b" " + producer_bytes):
                predicate_file.write_bytes(raw)
                with self.subTest(raw=raw[:40]), self.assertRaises(ColdReceipt):
                    verify_fixture(quarantine, compiled)

    def test_extra_payload_entries_and_evidence_change_revoke_grant(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            quarantine, compiled, _, _ = fixture(Path(temporary).resolve())
            grant = verify_fixture(quarantine, compiled)
            extra = quarantine / "roots" / "0" / "extra"
            extra.write_bytes(b"unbound")
            with self.assertRaisesRegex(ColdReceipt, "quarantine_manifest_mismatch"):
                grant.require_current()
            extra.unlink()
            grant.require_current()
            (quarantine / "roots" / "1" / "predicate.json").write_bytes(b"changed")
            with self.assertRaisesRegex(ColdReceipt, "quarantine_payload_changed"):
                grant.require_current()


if __name__ == "__main__":
    unittest.main()

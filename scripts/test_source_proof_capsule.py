"""Typed Semver proof capsule boundary tests."""

import base64
import json
import unittest

import source_proof_capsule as proof


class ProofCapsuleTests(unittest.TestCase):
    def test_projection_is_closed_and_hash_stable(self):
        encoded = (json.dumps(proof.PROJECTION_DOCUMENT, indent=2, sort_keys=True) + "\n").encode()
        self.assertEqual(proof.sha(encoded), proof.PROJECTION_SHA256)
        self.assertEqual(sum(item["kind"] == "canonical_asset" for item in proof.PROJECTION.values()), 3)
        self.assertEqual(sum(item["kind"] == "source_member" for item in proof.PROJECTION.values()), 3)
        self.assertEqual(sum(item["kind"] == "inline_bytes" for item in proof.PROJECTION.values()), 15)
        self.assertEqual(proof.PROJECTION_DOCUMENT["decoded_inline_bytes"], 900623)

    def test_json_and_base64_proof_encodings_are_canonical(self):
        self.assertEqual(proof.strict_json('{"a": 1}'), {"a": 1})
        with self.assertRaises(ValueError):
            proof.strict_json('{"a": 1, "a": 2}')
        encoded = base64.b64encode(b"proof").decode()
        self.assertEqual(proof._decode(encoded, "proof"), b"proof")
        for bad in (encoded + "\n", encoded + "=", "not base64"):
            with self.assertRaises(ValueError):
                proof._decode(bad, "proof")

    def test_asset_record_binds_name_digest_size_and_bytes(self):
        data = b"asset"
        digest = proof.sha(data)
        record = {"name": "asset", "sha256": digest, "size_bytes": len(data)}
        proof._verify_asset_record(record, "asset", data, digest, len(data))
        for mutation in ({**record, "name": "other"}, {**record, "sha256": "0" * 64},
                         {**record, "size_bytes": 0}, {"name": "asset"}):
            with self.assertRaises(ValueError):
                proof._verify_asset_record(mutation, "asset", data, digest, len(data))


if __name__ == "__main__":
    unittest.main()

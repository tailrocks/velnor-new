"""Adversarial public receipt policy and complete inventory proof."""
import hashlib
import os
import sys
import subprocess
import struct
import dataclasses
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "velnor-actions-mise" / "src"))
import cache_receipt as receipt
import cache_receipt_manifest as inventory
from cache_receipt_manifest import manifest
from cache_receipt_policy import ReceiptPolicy, qualified_policy
from cache_receipt_common import ColdReceipt, strict_json, secure_read
import cache_receipt_policy as policy_owner


def clean(root):
    if sys.platform == "darwin":
        subprocess.run(["/usr/bin/xattr", "-cr", str(root)], check=True)


def layout(roots=("tools",), optional=()):
    evidence_root = "cache-receipts/" + "c" * 64
    return dict(schema=1, sdk_paths=tuple("${{ runner.temp }}/velnor/" + root
                                        for root in (*roots, evidence_root)),
                payload_roots=roots, optional_roots=optional,
                payload_indices=tuple(range(len(roots))), evidence_index=len(roots),
                evidence_root=evidence_root,
                evidence_files=("manifest.json", "predicate.json", "bundle.sigstore.json"))


def policy():
    return ReceiptPolicy(policy_owner._POLICY_AUTHORITY,
        signer_uri="https://github.com/o/r/.github/workflows/pure.yml@" + "a" * 40,
        signer_digest="a" * 40, caller_uri="https://github.com/o/r/.github/workflows/ci.yml@refs/heads/main",
        caller_digest="b" * 40, repository="o/r", repository_id="123", source_ref="refs/heads/main",
        source_sha="b" * 40, role="tools", cache_key="exact", descriptor_sha256="c" * 64,
        helper_sha256="d" * 64, catalog_sha256="e" * 64, policy_sha256="f" * 64,
        predicate_type="https://velnor.dev/cache-producer/v1", producer_job_name="pure",
        allowed_roots=("tools",), gh_sha256="1" * 64, trusted_root_sha256="2" * 64,
        pure_callee_qualified=True, protected_default_push_qualified=True,
        public_attestation_qualified=True, transport_layout=layout())


def replace(original, **values):
    record = original.source_record()
    record.update(values)
    return ReceiptPolicy(policy_owner._POLICY_AUTHORITY, **record)


def evidence(data):
    p = policy()
    certificate = {
        "buildSignerURI": p.signer_uri, "buildSignerDigest": p.signer_digest,
        "subjectAlternativeName": p.signer_uri, "sourceRepositoryURI": "https://github.com/o/r",
        "sourceRepositoryDigest": p.source_sha, "sourceRepositoryRef": p.source_ref,
        "sourceRepositoryIdentifier": "123", "buildConfigURI": p.caller_uri,
        "buildConfigDigest": p.caller_digest, "issuer": "https://token.actions.githubusercontent.com",
        "buildTrigger": "push", "runnerEnvironment": "github-hosted",
        "sourceRepositoryVisibilityAtSigning": "public",
        "runInvocationURI": "https://github.com/o/r/actions/runs/7/attempts/2"}
    fields = ("role", "cache_key", "descriptor_sha256", "helper_sha256", "catalog_sha256",
              "policy_sha256", "repository_id", "source_sha")
    predicate = {key: getattr(p, key) for key in fields}
    digest = hashlib.sha256(data).hexdigest()
    predicate.update(schema=1, run_id=7, run_attempt=2, manifest_sha256=digest)
    statement = {"_type": "https://in-toto.io/Statement/v1", "predicateType": p.predicate_type,
                 "predicate": predicate, "subject": [{"name": "manifest.json",
                                                        "digest": {"sha256": digest}}]}
    return [{"verificationResult": {"signature": {"certificate": certificate}, "statement": statement}}]


class ReceiptTests(unittest.TestCase):
    def test_closed_factory_and_missing_qualification_are_cold(self):
        with self.assertRaises(ColdReceipt):
            qualified_policy("attacker-descriptor")
        for field in ("pure_callee_qualified", "protected_default_push_qualified",
                      "public_attestation_qualified"):
            with self.assertRaises(ColdReceipt):
                replace(policy(), **{field: False}).require_qualified()
        for fields in ({"repository_id": "01"}, {"repository_id": "0"},
                       {"caller_digest": "a" * 40}, {"source_ref": "refs/heads/"},
                       {"signer_uri": policy().signer_uri.replace("a" * 40, "refs/heads/main")}):
            with self.subTest(fields=fields), self.assertRaises(ColdReceipt):
                replace(policy(), **fields).require_qualified()

    def test_caller_policy_constructor_projection_and_replace_cannot_mint(self):
        with self.assertRaisesRegex(ColdReceipt, "receipt_policy_authority"):
            ReceiptPolicy(**policy().source_record())
        with self.assertRaisesRegex(ColdReceipt, "receipt_policy_authority"):
            dataclasses.replace(policy(), role="attacker-role")
        forged = object.__new__(ReceiptPolicy)
        for name, value in policy().source_record().items():
            object.__setattr__(forged, name, value)
        with self.assertRaisesRegex(ColdReceipt, "receipt_policy_authority"):
            forged.require_qualified()
        with self.assertRaises(ColdReceipt):
            qualified_policy(policy().source_record())
        class CallerPolicy:
            def require_qualified(self):
                return None
        with self.assertRaisesRegex(ColdReceipt, "receipt_policy_authority"):
            receipt.verify_live_payload("/managed", "/bundle", None, CallerPolicy())

    def test_compiled_capsule_selector_and_source_identity_are_closed(self):
        original = policy()
        capsule = {"descriptor_sha256": original.descriptor_sha256,
                   "producer_helper_closure_sha256": original.helper_sha256,
                   "callee_commit_sha": original.signer_digest,
                   "policy": original.source_record()}
        with patch.object(policy_owner, "_COMPILED_POLICY_CAPSULE", capsule):
            minted = qualified_policy(original.descriptor_sha256)
            minted.require_qualified()
            with self.assertRaisesRegex(ColdReceipt, "receipt_policy_capsule_identity"):
                qualified_policy("wrong-selector")
        for key in ("producer_helper_closure_sha256", "callee_commit_sha"):
            forged = dict(capsule, **{key: "0" * 64})
            with patch.object(policy_owner, "_COMPILED_POLICY_CAPSULE", forged):
                with self.assertRaisesRegex(ColdReceipt, "receipt_policy_capsule_identity"):
                    qualified_policy(original.descriptor_sha256)

    def test_shared_json_rejects_duplicate_and_nonfinite_numbers(self):
        for data in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}', b'{"x":1e999}'):
            with self.subTest(data=data), self.assertRaises(ColdReceipt):
                strict_json(data)

    def test_live_full_grant_only_factory_and_current_inventory(self):
        full = replace(policy(), role="tool-full", allowed_roots=receipt._FULL_ROOTS,
                       optional_roots=receipt._FULL_OPTIONAL,
                       transport_layout=layout(receipt._FULL_ROOTS, receipt._FULL_OPTIONAL))
        with self.assertRaisesRegex(ColdReceipt, "full_tool_authority"):
            receipt.VerifiedFullToolPayload(None, "/managed", b"manifest", full)
        with patch.object(receipt, "inventory_exact_roots", return_value=b"manifest"), \
                patch.object(receipt, "_prove_manifest") as prove:
            grant = receipt.verify_live_payload("/managed", "/bundle", None, full)
            grant.require_current()
            prove.assert_called_once()
        self.assertEqual(grant.root, "/managed")
        self.assertEqual(grant.manifest_bytes, b"manifest")
        self.assertEqual(grant.role, "tool-full")
        with self.assertRaisesRegex(ColdReceipt, "full_tool_grant_immutable"):
            grant._manifest = b"forged"
        with patch.object(receipt, "inventory_exact_roots", return_value=b"changed"):
            with self.assertRaisesRegex(ColdReceipt, "full_tool_payload_changed"):
                grant.require_current()
        with self.assertRaisesRegex(ColdReceipt, "full_tool_recipe_unqualified"):
            receipt.verify_live_payload("/managed", "/bundle", None, policy())

    def test_unverified_json_is_never_admitted(self):
        with self.assertRaisesRegex(ColdReceipt, "unverified_bundle"):
            receipt._admit(evidence(b"manifest"), b"manifest", policy(), None)

    def test_correct_signed_identity_only_admits_after_attempt_join(self):
        value = receipt._GhVerified(evidence(b"manifest"), receipt._VERIFIED)
        with patch.object(receipt, "_attempt") as attempt:
            admitted = receipt._admit(value, b"manifest", policy(), None)
        self.assertEqual(admitted.run_attempt, 2)
        attempt.assert_called_once()

    def test_shadow_source_sibling_caller_invocation_and_private_fail(self):
        mutations = {"sourceRepositoryRef": "refs/pull/1/merge", "buildSignerURI": "sibling",
                     "buildSignerDigest": "0" * 40, "buildConfigURI": "wrong-caller",
                     "sourceRepositoryIdentifier": "124", "sourceRepositoryDigest": "0" * 40,
                     "buildTrigger": "pull_request", "runnerEnvironment": "self-hosted",
                     "runInvocationURI": "https://github.com/o/r/actions/runs/7/attempts/1",
                     "sourceRepositoryVisibilityAtSigning": "private", "extensions": {}}
        for key, replacement in mutations.items():
            value = evidence(b"manifest")
            value[0]["verificationResult"]["signature"]["certificate"][key] = replacement
            with self.subTest(key=key), self.assertRaises(ColdReceipt), patch.object(receipt, "_attempt"):
                receipt._admit(receipt._GhVerified(value, receipt._VERIFIED), b"manifest", policy(), None)

    def test_unknown_fields_boolean_schema_and_corruption_fail(self):
        for key, replacement in (("schema", True), ("run_id", True), ("run_attempt", 0),
                                 ("role", "workload"), ("arbitrary", "command")):
            value = evidence(b"manifest")
            value[0]["verificationResult"]["statement"]["predicate"][key] = replacement
            with self.subTest(key=key), self.assertRaises(ColdReceipt), patch.object(receipt, "_attempt"):
                receipt._admit(receipt._GhVerified(value, receipt._VERIFIED), b"manifest", policy(), None)
        with self.assertRaises(ColdReceipt):
            receipt._predicate(evidence(b"manifest")[0]["verificationResult"]["statement"], b"changed", policy())
        with self.assertRaises(ColdReceipt):
            strict_json(b'{"key":1,"key":2}')

    def test_exact_attempt_success_complete_pagination_required(self):
        predicate = evidence(b"manifest")[0]["verificationResult"]["statement"]["predicate"]
        run = {"id": 7, "run_attempt": 2, "head_sha": "b" * 40, "head_branch": "main",
               "event": "push", "status": "completed", "conclusion": "success",
               "repository": {"id": 123}}
        job = {"id": 8, "name": "pure", "run_id": 7, "run_attempt": 2,
               "status": "completed", "conclusion": "success"}
        class Api:
            def __init__(self, metadata, jobs):
                self.metadata, self.jobs = metadata, jobs
            def api(self, endpoint, paginate=False):
                self_endpoint = "repos/o/r/actions/runs/7/attempts/2"
                if endpoint != self_endpoint + ("/jobs?per_page=100" if paginate else ""):
                    raise AssertionError(endpoint)
                return self.jobs if paginate else self.metadata
        receipt._attempt(Api(run, [{"total_count": 1, "jobs": [job]}]), policy(), predicate)
        for changed in (dict(job, run_attempt=1), dict(job, conclusion="failure")):
            with self.assertRaises(ColdReceipt):
                receipt._attempt(Api(run, [{"total_count": 1, "jobs": [changed]}]), policy(), predicate)
        for pages in ([{"total_count": 2, "jobs": [job]}],
                      [{"total_count": 2, "jobs": [job, job]}], []):
            with self.assertRaises(ColdReceipt):
                receipt._attempt(Api(run, pages), policy(), predicate)
        with self.assertRaises(ColdReceipt):
            receipt._attempt(Api(dict(run, event="pull_request"), []), policy(), predicate)

    @patch("source_archive_inventory_leaf.metadata_records", return_value=[])
    def test_manifest_binds_content_mode_paths_and_link_targets(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "tools").mkdir()
            file = root / "tools" / "gh"
            file.write_bytes(b"original")
            clean(root)
            original = manifest(root, ("tools",))
            file.write_bytes(b"changed")
            self.assertNotEqual(original, manifest(root, ("tools",)))
            changed = manifest(root, ("tools",))
            file.chmod(0o755)
            self.assertNotEqual(changed, manifest(root, ("tools",)))
            link = root / "tools" / "link"
            link.symlink_to("gh")
            clean(root)
            linked = manifest(root, ("tools",))
            link.unlink()
            link.symlink_to("../../escape")
            with self.assertRaises(ColdReceipt):
                manifest(root, ("tools",))
            self.assertIn(b'"target":"gh"', linked)

    @patch("source_archive_inventory_leaf.metadata_records", return_value=[])
    def test_unknown_hardlink_and_linked_ancestor_rejected(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve() / "payload"
            root.mkdir()
            (root / "tools").mkdir()
            file = root / "tools" / "gh"
            file.write_bytes(b"binary")
            (root / "unknown").write_bytes(b"secret")
            with self.assertRaisesRegex(ColdReceipt, "payload_unknown_path"):
                manifest(root, ("tools",))
            (root / "unknown").unlink()
            os.link(file, root / "tools" / "hardlink")
            self.assertIn(b"hardlink", manifest(root, ("tools",)))
            outside = root.parent / "outside-hardlink"
            os.link(file, outside)
            with self.assertRaisesRegex(ColdReceipt, "payload_hardlink_external_or_missing"):
                manifest(root, ("tools",))
            outside.unlink()
            (root / "tools" / "hardlink").unlink()
            link = root.parent / "parent-link"
            link.symlink_to(root.parent)
            with self.assertRaisesRegex(ColdReceipt, "payload_root_unqualified"):
                manifest(link / "payload", ("tools",))

    def test_actual_metadata_rejected_on_entry(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            file = root / "file"
            file.write_bytes(b"binary")
            attribute = "com.apple.velnor-receipt-test" if sys.platform == "darwin" else "user.velnor"
            if sys.platform == "darwin":
                subprocess.run(["/usr/bin/xattr", "-w", attribute, "untrusted", str(file)], check=True)
            else:
                os.setxattr(file, attribute, b"untrusted")
            directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
            try:
                with self.assertRaisesRegex(ColdReceipt, "payload_unsupported_metadata"):
                    inventory._entry(directory, "file", "file", root)
            finally:
                os.close(directory)

    def test_bundle_fifo_symlink_hardlink_and_symlink_ancestor_reject(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            fifo = root / "fifo"
            os.mkfifo(fifo)
            with self.assertRaisesRegex(ColdReceipt, "bundle_not_regular"):
                secure_read(fifo)
            file = root / "bundle"
            file.write_bytes(b"public-bundle")
            self.assertEqual(secure_read(file), b"public-bundle")
            link = root / "link"
            link.symlink_to(file)
            with self.assertRaises(OSError):
                secure_read(link)
            link.unlink()
            os.link(file, link)
            with self.assertRaisesRegex(ColdReceipt, "bundle_not_regular"):
                secure_read(file)
            link.unlink()
            link.symlink_to(root, target_is_directory=True)
            with self.assertRaises(OSError):
                secure_read(link / "bundle")

    @patch("source_archive_inventory_leaf.metadata_records", return_value=[])
    def test_lexical_alias_is_rejected_before_root_lookup(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "good" / "payload").mkdir(parents=True)
            (root / "evil" / "deep").mkdir(parents=True)
            (root / "good" / "link").symlink_to(root / "evil" / "deep")
            alias = str(root / "good" / "link") + "/../payload"
            with self.assertRaisesRegex(ColdReceipt, "payload_root_alias"):
                inventory._root_descriptor(alias)
            with self.assertRaisesRegex(ColdReceipt, "payload_root_alias"):
                secure_read(alias + "/bundle.json")

    @patch("source_archive_inventory_leaf.metadata_records", return_value=[])
    def test_exact_roots_exclude_siblings_and_bind_optional_absence(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "cargo" / "registry").mkdir(parents=True)
            (root / "cargo" / "registry" / "index").write_bytes(b"public")
            (root / "cargo" / "credentials").write_bytes(b"secret")
            (root / "receipts").mkdir()
            (root / "receipts" / "bundle.json").write_bytes(b"public")
            roots = ("cargo/registry", "cargo/.crates")
            first = inventory.inventory_exact_roots(root, roots, ("cargo/.crates",))
            self.assertNotIn(b"credentials", first)
            self.assertNotIn(b"receipts", first)
            self.assertIn(b'"kind":"missing"', first)
            (root / "cargo" / ".crates").write_bytes(b"metadata")
            self.assertNotEqual(first, inventory.inventory_exact_roots(root, roots, ("cargo/.crates",)))
            (root / "cargo" / "registry" / "additional").write_bytes(b"included")
            self.assertIn(b"additional", inventory.inventory_exact_roots(root, roots, ("cargo/.crates",)))

    @patch("source_archive_inventory_leaf.metadata_records", return_value=[])
    def test_true_appledouble_refused_and_ordinary_dotunderscore_preserved(self, _metadata):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "tools").mkdir()
            file = root / "tools" / "._data"
            file.write_bytes(b"ordinary public filename")
            self.assertIn(b"._data", manifest(root, ("tools",)))
            header = struct.pack(">II16sH", 0x00051607, 0x00020000, b"\0" * 16, 1)
            canary = b"private resource fork"
            file.write_bytes(header + struct.pack(">III", 2, 38, len(canary)) + canary)
            with self.assertRaisesRegex(ColdReceipt, "payload_appledouble_metadata"):
                manifest(root, ("tools",))


if __name__ == "__main__":
    unittest.main()

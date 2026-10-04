"""Offline content checks and diagnostic owner-boundary negatives; no live authority."""
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch
import zipfile


ROOT = Path(__file__).resolve().parents[1] / "src"
RUST = ROOT.parents[1] / "velnor-actions-rust"
CONTRACT_FIXTURE = {"__name__": "prepared_content_fixture",
                    "__file__": str(RUST / "tests/source_intent_contract_test.py")}
exec(compile(Path(CONTRACT_FIXTURE["__file__"]).read_bytes(),
             CONTRACT_FIXTURE["__file__"], "exec"), CONTRACT_FIXTURE)


class PreparedArtifactInputTests(unittest.TestCase):
    def setUp(self):
        (self.ns, self.approved, self.descriptor, self.evidence,
         self.live, self.pure) = CONTRACT_FIXTURE["fixture"]()
        source = ROOT / "release_prepared_artifact_input.py"
        exec(compile(source.read_bytes(), str(source), "exec"), self.ns)
        self.archive = CONTRACT_FIXTURE["FIXTURE"]["package"]()[1]
        self.raw = self.encode()
        # Portable input claims for codec diagnostics, never a compiled grant.
        self.binding = {"approved": self.approved, "repository": self.approved["repository"],
            "source_sha": self.approved["source_sha"], "workflow": ".github/workflows/release.yml",
            "workflow_sha": "d" * 40, "ref": "refs/heads/main", "run_id": "123",
            "attempt": "1", "producer_job": "release-package",
            "producer_helper_sha256": "a" * 64, "artifact_id": "789",
            "raw_zip_sha256": "b" * 64, "inner_sha256": hashlib.sha256(self.raw).hexdigest(),
            "destination": "/diagnostic/release-prepared-input"}

    def encode(self, evidence=None, archive=None, extra=None, compression=zipfile.ZIP_STORED):
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w", compression=compression) as output:
            value = self.evidence if evidence is None else evidence
            output.writestr("evidence.json", value if type(value) is bytes
                            else json.dumps(value).encode())
            output.writestr("crates/demo-1.0.0.crate",
                            self.archive if archive is None else archive)
            if extra is not None:
                output.writestr(extra, b"extra")
        return buffer.getvalue()

    def decode(self, raw=None, binding=None):
        return self.ns["decode_prepared_package"](
            self.raw if raw is None else raw, self.binding if binding is None else binding)

    def rebind(self, raw):
        binding = copy.deepcopy(self.binding)
        binding["inner_sha256"] = hashlib.sha256(raw).hexdigest()
        return binding

    def test_pure_decode_preserves_original_bytes_and_grants_no_handle(self):
        evidence, archives = self.decode()
        self.assertEqual(evidence, self.evidence)
        self.assertEqual(archives, {"demo": self.archive})
        for value in (evidence, archives, self.binding, self.raw, self.pure, self.live):
            with self.subTest(value=type(value).__name__), self.assertRaisesRegex(
                    self.ns["ReconcileError"], "prepared_artifact_authority"):
                self.ns["authenticated_prepared_package"](value)

    def test_standalone_loader_denies_before_any_filesystem_or_source_access(self):
        def unexpected(*_args):
            self.fail("cold loader accessed unqualified input")
        with patch.dict(self.ns, {"_prepared_read_content": unexpected,
                                  "load_authenticated_source_snapshot": unexpected}):
            with self.assertRaisesRegex(self.ns["ReconcileError"], "compiled_input_unqualified"):
                self.ns["load_authenticated_prepared_package"]()

    def test_claims_cannot_construct_or_substitute_a_handle(self):
        constructor = self.ns["_ActualPreparedHandle"]
        with self.assertRaisesRegex(self.ns["ReconcileError"], "prepared_artifact_authority"):
            constructor(object(), self.raw, {"demo": self.archive}, self.evidence,
                        self.pure, self.binding)

    def test_transport_types_and_fields_are_closed(self):
        for field in ("run_id", "attempt", "artifact_id", "inner_sha256",
                      "producer_helper_sha256", "raw_zip_sha256", "source_sha", "workflow_sha"):
            for value in (True, 1, "01", "A" * 64):
                binding = copy.deepcopy(self.binding)
                binding[field] = value
                with self.subTest(field=field, value=value), self.assertRaises(
                        self.ns["ReconcileError"]):
                    self.decode(binding=binding)
        for change in (lambda value: value.update(extra=True),
                       lambda value: value.pop("approved"),
                       lambda value: value.update(workflow="other.yml"),
                       lambda value: value.update(producer_job="release-prepared-package"),
                       lambda value: value.update(destination="/a/../b"),
                       lambda value: value.update(repository="other/repo")):
            binding = copy.deepcopy(self.binding)
            change(binding)
            with self.assertRaises(self.ns["ReconcileError"]):
                self.decode(binding=binding)

    def test_inner_digest_and_archive_digest_are_independent(self):
        with self.assertRaisesRegex(self.ns["ReconcileError"], "inner_digest"):
            self.decode(raw=self.raw + b"changed")
        changed = self.encode(archive=self.archive + b"changed")
        with self.assertRaisesRegex(self.ns["ReconcileError"], "archive_digest"):
            self.decode(changed, self.rebind(changed))

    def test_exact_member_set_compression_and_member_type(self):
        for extra in ("other.json", "../escape", "crates/extra-1.0.0.crate"):
            raw = self.encode(extra=extra)
            with self.subTest(extra=extra), self.assertRaisesRegex(
                    self.ns["ReconcileError"], "exact_members"):
                self.decode(raw, self.rebind(raw))
        raw = self.encode(compression=zipfile.ZIP_DEFLATED)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "zip_member"):
            self.decode(raw, self.rebind(raw))
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as output:
            info = zipfile.ZipInfo("evidence.json")
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            output.writestr(info, b"target")
        raw = buffer.getvalue()
        with self.assertRaisesRegex(self.ns["ReconcileError"], "zip_member"):
            self.decode(raw, self.rebind(raw))

    def test_shared_actual_schema_rejects_extra_fields_and_boolean_schema(self):
        for change in (lambda value: value.update(extra=True),
                       lambda value: value.update(schema=True),
                       lambda value: value["source_snapshot"].update(source_sha="c" * 40)):
            evidence = copy.deepcopy(self.evidence)
            change(evidence)
            raw = self.encode(evidence=evidence)
            with self.assertRaises(self.ns["ReconcileError"]):
                self.decode(raw, self.rebind(raw))

    def test_binary_local_and_central_nul_aliases_fail_for_every_member(self):
        members = {"evidence.json": json.dumps(self.evidence).encode(),
                   "crates/demo-1.0.0.crate": self.archive}
        for target in members:
            buffer = io.BytesIO()
            placeholder = target + "!"
            with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_STORED) as output:
                for name, content in members.items():
                    output.writestr(placeholder if name == target else name, content)
            original = buffer.getvalue()
            before = placeholder.encode("ascii")
            after = target.encode("ascii") + b"\x00"
            self.assertEqual(len(before), len(after))
            self.assertEqual(original.count(before), 2)
            raw = original.replace(before, after)
            with zipfile.ZipFile(io.BytesIO(raw)) as archive:
                member = next(item for item in archive.infolist() if item.filename == target)
                self.assertEqual(member.orig_filename, target + "\x00")
                self.assertEqual(archive.read(member), members[target])
            with self.subTest(member=target), self.assertRaisesRegex(
                    self.ns["ReconcileError"], "zip_member"):
                self.decode(raw, self.rebind(raw))

    def test_actual_zip_evidence_invalid_utf8_and_huge_integer_fail_closed(self):
        canonical = json.dumps(self.evidence).encode()
        huge_integer = canonical.replace(b'"schema": 1',
                                         b'"schema": ' + b"1" * 5000, 1)
        for evidence in (b"\xff", huge_integer):
            raw = self.encode(evidence=evidence)
            with self.subTest(kind="utf8" if evidence == b"\xff" else "integer"):
                with self.assertRaises(self.ns["ReconcileError"]):
                    self.decode(raw, self.rebind(raw))

    def test_actual_valid_deep_zip_json_fails_shared_schema_type_check(self):
        canonical = json.dumps(self.evidence).encode()
        for depth in (2000, 10000, 20000):
            nested = b"[" * depth + b"0" + b"]" * depth
            evidence = canonical.replace(b'"schema": 1', b'"schema": ' + nested, 1)
            # Actual stdlib may accept deep JSON. Its value remains the wrong schema type.
            decoded = self.ns["decode_json"](evidence)
            self.assertIs(type(decoded["schema"]), list)
            raw = self.encode(evidence=evidence)
            with self.subTest(depth=depth), self.assertRaisesRegex(
                    self.ns["ReconcileError"], "prepared_evidence_schema"):
                self.decode(raw, self.rebind(raw))

    def test_owned_read_pins_directory_and_rejects_links_and_extra_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            directory = root / "release-prepared-input"
            directory.mkdir(parents=True)
            path = directory / "prepared.zip"
            path.write_bytes(self.raw)
            binding = copy.deepcopy(self.binding)
            binding["destination"] = str(directory)
            with patch.dict(os.environ, {"GITHUB_WORKSPACE": str(root)}):
                self.assertEqual(self.ns["_prepared_read_content"](binding), self.raw)
                (directory / "extra").write_bytes(b"extra")
                with self.assertRaisesRegex(self.ns["ReconcileError"], "layout"):
                    self.ns["_prepared_read_content"](binding)
                (directory / "extra").unlink()
                other = root / "original.zip"
                path.rename(other)
                path.symlink_to(other)
                with self.assertRaises(OSError):
                    self.ns["_prepared_read_content"](binding)

    def test_diagnostic_getter_cannot_promote_pure_source_content(self):
        # Owner-boundary mock only: no positive authenticated source is invented.
        with patch.dict(self.ns, {
                "_compiled_prepared_artifact_input": lambda: self.binding,
                "_prepared_read_content": lambda _binding: self.raw,
                "load_authenticated_source_snapshot": lambda: self.pure}):
            with self.assertRaisesRegex(self.ns["ReconcileError"], "source_artifact_authority"):
                self.ns["load_authenticated_prepared_package"]()

    def test_workspace_identity_and_pinned_download_path_are_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            binding = copy.deepcopy(self.binding)
            binding["destination"] = str(root / "release-prepared-input")
            with patch.dict(os.environ, {"GITHUB_WORKSPACE": str(root)}):
                binding["destination"] = str(root / "caller-selected")
                with self.assertRaisesRegex(self.ns["ReconcileError"], "pinned_destination"):
                    self.ns["_prepared_input_path"](binding)
            alias = root / "workspace-alias"
            alias.symlink_to(root, target_is_directory=True)
            with patch.dict(os.environ, {"GITHUB_WORKSPACE": str(alias)}):
                with self.assertRaisesRegex(self.ns["ReconcileError"], "workspace_noncanonical"):
                    self.ns["_prepared_input_path"](binding)
            with patch.dict(os.environ, {"GITHUB_WORKSPACE": "relative/workspace"}):
                with self.assertRaisesRegex(self.ns["ReconcileError"], "prepared_artifact_workspace"):
                    self.ns["_prepared_input_path"](binding)


if __name__ == "__main__":
    unittest.main()

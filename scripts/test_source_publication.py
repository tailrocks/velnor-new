"""Source proof tests with synthetic closed records and no network access."""

import copy
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import source_publication as S
import source_publication_records as records


TARGET = "c57c700459bbe1549fe7eedcb7d8689585c38986"
BASE = {"mise": "bc11f90c74eba23bf0d7350efb540e62fb7d9ffd",
        "mbx-action": "1687e54eb349cadf61fa38b5813a77875489e8e6"}
UPSTREAM = {"mise": "jdx/mise", "mbx-action": "jdx/mr-boxington-action"}


def fixture(role="mise"):
    lock = "package-lock.json" if role == "mbx-action" else "Cargo.lock"
    payloads = {lock: b"locked", "LICENSE": b"license"}
    if role == "mbx-action":
        payloads["dist/index.js"] = b"action bundle"
    files = {name: (b"100644", S.git_object("blob", data))
             for name, data in payloads.items()}
    tree = S.git_object("tree", b"fixture tree object")
    raw = (f"tree {tree}\nparent {BASE[role]}\n"
        "author Reviewer <reviewer@example.test> 1 +0000\n"
        "committer Reviewer <reviewer@example.test> 1 +0000\n\nsource change\n\n"
        "Signed-off-by: Reviewer <reviewer@example.test>\n"
        "Co-authored-by: Codex <codex@openai.com>\n").encode()
    commit = S.git_object("commit", raw)
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:") as archive:
        for name, data in sorted(payloads.items()):
            entry = tarfile.TarInfo("source/" + name)
            entry.mode, entry.size = 0o644, len(data)
            archive.addfile(entry, io.BytesIO(data))
    archive_data, patch_data = buffer.getvalue(), b"exact reviewed patch"
    receipt = {"schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": role,
        "upstream_repository": "https://github.com/" + UPSTREAM[role],
        "upstream_base_commit": BASE[role], "source_commit": commit,
        "source_tree": tree, "source_archive": {"name": "source.tar", "sha256": S.sha(archive_data)},
        "base_patch": {"name": "base.patch", "sha256": S.sha(patch_data)},
        "lockfile": {"path": lock, "sha256": S.sha(payloads[lock])},
        "license_files": {"LICENSE": S.sha(payloads["LICENSE"])},
        "required_hosts": [] if role == "mbx-action" else S.HOSTS,
        "publication": None, "behavioral_qualification": None,
        "signed_build_provenance": None}
    if role == "mbx-action":
        receipt["action_bundle"] = {"path": "dist/index.js", "sha256": S.sha(payloads["dist/index.js"])}
    receipt_data = json.dumps(receipt).encode()
    revision = records._record(
        role, commit, tree, BASE[role], UPSTREAM[role], "SourcePrefixFilesV1",
        S.sha(archive_data), len(archive_data), S.sha(patch_data), S.sha(raw),
        None, S.sha(receipt_data))
    approved = {"base": BASE[role], "tree": tree, "upstream": UPSTREAM[role],
        "commit": commit, "raw_commit_sha256": S.sha(raw),
        "source-receipt.json": S.sha(receipt_data), "revision": revision}

    def git(repository, *arguments):
        if arguments[:2] == ("cat-file", "commit"):
            return raw
        if arguments[:2] == ("cat-file", "tree"):
            return b"fixture tree object"
        if arguments[0] == "merge-base":
            return b""
        if arguments[0] == "ls-tree":
            return b"".join(mode + b" blob " + oid.encode() + b"\t" + name.encode() + b"\0"
                            for name, (mode, oid) in files.items())
        if arguments[0] == "diff":
            return patch_data
        raise AssertionError(arguments)
    return approved, {"source.tar": archive_data, "base.patch": patch_data,
                      "source-receipt.json": receipt_data}, raw, receipt, payloads, files, git


def record_patch(approved):
    revision = approved["revision"]
    return patch.object(records, "REVISIONS", {(revision.role.value, revision.source_commit): revision})


def replace_revision(revision, **changes):
    values = {field: getattr(revision, field) for field in revision.__dataclass_fields__}
    values.update(changes)
    return records._record(values["role"].value, values["source_commit"], values["source_tree"],
        values["upstream_base_commit"], values["upstream_repository"], values["archive_kind"].value,
        values["source_archive_sha256"], values["source_archive_size"], values["base_patch_sha256"],
        values["raw_commit_sha256"], values["owner_manifest_sha256"], values["source_receipt_sha256"])


class SourceProofTests(unittest.TestCase):
    def test_reviewed_source_only_closure_for_both_roles(self):
        for role in ("mise", "mbx-action"):
            approved, assets, raw, _, _, _, git = fixture(role)
            revision = approved["revision"]
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                for name, data in assets.items():
                    (root / name).write_bytes(data)
                with record_patch(approved), patch.object(S, "git", side_effect=git):
                    manifest, result = S.prepare(root, root, role, revision.source_ref,
                        revision.source_commit, TARGET)
                self.assertEqual(set(result), S.STAGE_NAMES | {"source.commit", "source-publication.json"})
                self.assertEqual(result["source.commit"], raw)
                self.assertEqual(manifest["tag_target"], TARGET)
                self.assertEqual(manifest["raw_commit"]["git_object_sha1"], revision.source_commit)
                self.assertIsNone(manifest["raw_commit"]["cryptographic_signature"])
                self.assertIsNone(manifest["behavioral_qualification"])
                self.assertIsNone(manifest["signed_build_provenance"])

    def test_only_exact_reviewed_source_and_target(self):
        approved, _, _, _, _, _, _ = fixture()
        revision = approved["revision"]
        with record_patch(approved):
            for role, ref, commit, target in (
                    ("mbx-action", revision.source_ref, revision.source_commit, TARGET),
                    ("mise", "refs/heads/main", revision.source_commit, TARGET),
                    ("mise", revision.source_ref, "0" * 40, TARGET),
                    ("mise", revision.source_ref, revision.source_commit, revision.source_commit)):
                with self.assertRaises(ValueError):
                    S.source_identity(role, ref, commit, target)

    def test_raw_commit_hash_and_dco_authority(self):
        approved, _, raw, _, _, _, git = fixture()
        revision = approved["revision"]
        with patch.object(S, "git", side_effect=git):
            proof, signoffs = S.raw_commit_proof(Path("repository"), revision)
        self.assertEqual(proof, raw)
        self.assertEqual(signoffs, ["Reviewer <reviewer@example.test>"])
        for change in (replace_revision(revision, source_commit="0" * 40),
                       replace_revision(revision, source_tree="0" * 40),
                       replace_revision(revision, raw_commit_sha256="0" * 64)):
            with patch.object(S, "git", side_effect=git), self.assertRaises(ValueError):
                S.raw_commit_proof(Path("repository"), change)

    def test_receipt_must_be_closed_unqualified_source(self):
        approved, _, _, receipt, payloads, files, _ = fixture()
        revision = approved["revision"]
        mutations = [lambda r: r.update(schema=True), lambda r: r.update(behavioral_qualification={}),
            lambda r: r.update(publication={}), lambda r: r.update(signed_build_provenance={}),
            lambda r: r.update(upstream_base_commit="0" * 40), lambda r: r.update(extra=True),
            lambda r: r["license_files"].update(LICENSE="0" * 64),
            lambda r: r["lockfile"].update(sha256="0" * 64)]
        for mutation in mutations:
            candidate = copy.deepcopy(receipt)
            mutation(candidate)
            with self.assertRaises(ValueError):
                S.receipt_proof(candidate, revision, payloads, files)
        with self.assertRaises(ValueError):
            S.strict_json('{"schema":1,"schema":1}')

    def test_archive_cannot_change_or_omit_git_blobs(self):
        _, assets, _, _, _, files, _ = fixture()
        S.archive_proof(assets["source.tar"], files)
        for names in (["LICENSE"], ["LICENSE", "LICENSE"], ["../escape"]):
            buffer = io.BytesIO()
            with tarfile.open(fileobj=buffer, mode="w:") as archive:
                for name in names:
                    data = b"changed"
                    entry = tarfile.TarInfo("source/" + name)
                    entry.mode, entry.size = 0o644, len(data)
                    archive.addfile(entry, io.BytesIO(data))
            with self.assertRaises(ValueError):
                S.archive_proof(buffer.getvalue(), files)


if __name__ == "__main__":
    unittest.main()

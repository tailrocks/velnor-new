"""Isolated source-only publication fixtures; no network or source execution."""

import copy
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import source_publication as S


def fixture(role="mise"):
    lock = "package-lock.json" if role == "mbx-action" else "Cargo.lock"
    payloads = {lock: b"locked", "LICENSE": b"license"}
    if role == "mbx-action":
        payloads["dist/index.js"] = b"action bundle"
    files = {name: (b"100644", S.git_object("blob", data)) for name, data in payloads.items()}
    tree_data = b"fixture tree object"
    tree = S.git_object("tree", tree_data)
    raw = (f"tree {tree}\nparent {S.APPROVED[role]['base']}\n"
        "author Reviewer <reviewer@example.test> 1 +0000\n"
        "committer Reviewer <reviewer@example.test> 1 +0000\n\nsource change\n\n"
        "Signed-off-by: Reviewer <reviewer@example.test>\n"
        "Co-authored-by: Codex <codex@openai.com>\n").encode()
    approved = copy.deepcopy(S.APPROVED[role])
    approved.update(commit=S.git_object("commit", raw), tree=tree, raw_commit_sha256=S.sha(raw))
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:") as archive:
        for name, data in sorted(payloads.items()):
            entry = tarfile.TarInfo("source/" + name)
            entry.mode, entry.size = 0o644, len(data)
            archive.addfile(entry, io.BytesIO(data))
    archive_data, patch_data = buffer.getvalue(), b"exact reviewed patch"
    approved.update({"source.tar": S.sha(archive_data), "base.patch": S.sha(patch_data)})
    receipt = {"schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": role,
        "upstream_repository": "https://github.com/" + approved["upstream"],
        "upstream_base_commit": approved["base"], "source_commit": approved["commit"],
        "source_tree": tree, "source_archive": {"name": "source.tar", "sha256": S.sha(archive_data)},
        "base_patch": {"name": "base.patch", "sha256": S.sha(patch_data)},
        "lockfile": {"path": lock, "sha256": S.sha(payloads[lock])},
        "license_files": {"LICENSE": S.sha(payloads["LICENSE"])},
        "required_hosts": [] if role == "mbx-action" else S.HOSTS,
        "publication": None, "behavioral_qualification": None, "signed_build_provenance": None}
    if role == "mbx-action":
        receipt["action_bundle"] = {"path": "dist/index.js", "sha256": S.sha(payloads["dist/index.js"])}
    receipt_data = json.dumps(receipt).encode()
    approved["source-receipt.json"] = S.sha(receipt_data)
    def git(repository, *arguments):
        if arguments[:2] == ("cat-file", "commit"):
            return raw
        if arguments[:2] == ("cat-file", "tree"):
            return tree_data
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


class SourceProofTests(unittest.TestCase):
    def test_reviewed_source_only_closure_for_both_roles(self):
        for role in ("mise", "mbx-action"):
            approved, assets, raw, _, _, _, git = fixture(role)
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary).resolve()
                for name, data in assets.items():
                    (root / name).write_bytes(data)
                with patch.dict(S.APPROVED, {role: approved}), patch.object(S, "git", side_effect=git):
                    manifest, result = S.prepare(root, root, role,
                        "refs/heads/owned-source/" + role + "/" + approved["commit"], approved["commit"], S.TARGET)
                self.assertEqual(set(result), S.STAGE_NAMES | {"source.commit", "source-publication.json"})
                self.assertEqual(result["source.commit"], raw)
                self.assertEqual(manifest["tag_target"], S.TARGET)
                self.assertEqual(manifest["raw_commit"]["git_object_sha1"], approved["commit"])
                self.assertIsNone(manifest["raw_commit"]["cryptographic_signature"])
                self.assertIsNone(manifest["behavioral_qualification"])
                self.assertIsNone(manifest["signed_build_provenance"])
                self.assertEqual(manifest["source_receipt"]["status"], "STAGED_SOURCE_ONLY")

    def test_only_exact_reviewed_source_and_target(self):
        approved = S.APPROVED["mise"]
        source_ref = "refs/heads/owned-source/mise/" + approved["commit"]
        for role, ref, commit, target in (("mbx", source_ref, approved["commit"], S.TARGET),
            ("mise", "refs/heads/main", approved["commit"], S.TARGET),
            ("mise", source_ref, "0" * 40, S.TARGET),
            ("mise", source_ref, approved["commit"], approved["commit"])):
            with self.assertRaises(ValueError):
                S.source_identity(role, ref, commit, target)

    def test_raw_commit_hash_and_dco_authority(self):
        approved, _, raw, _, _, _, git = fixture()
        with patch.object(S, "git", side_effect=git):
            proof, signoffs = S.raw_commit_proof(Path("repository"), approved)
        self.assertEqual(proof, raw)
        self.assertEqual(signoffs, ["Reviewer <reviewer@example.test>"])
        for change in ({"commit": "0" * 40}, {"tree": "0" * 40}, {"raw_commit_sha256": "0" * 64}):
            invalid = dict(approved, **change)
            with patch.object(S, "git", side_effect=git), self.assertRaises(ValueError):
                S.raw_commit_proof(Path("repository"), invalid)

    def test_receipt_must_be_closed_unqualified_source(self):
        approved, _, _, receipt, payloads, files, _ = fixture()
        mutations = [lambda r: r.update(schema=True), lambda r: r.update(behavioral_qualification={}),
            lambda r: r.update(publication={}), lambda r: r.update(signed_build_provenance={}),
            lambda r: r.update(upstream_base_commit="0" * 40), lambda r: r.update(extra=True),
            lambda r: r["license_files"].update(LICENSE="0" * 64),
            lambda r: r["lockfile"].update(sha256="0" * 64)]
        for mutation in mutations:
            candidate = copy.deepcopy(receipt)
            mutation(candidate)
            with self.assertRaises(ValueError):
                S.receipt_proof(candidate, "mise", approved, payloads, files)
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

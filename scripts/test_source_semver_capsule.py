"""Adversarial tests for the strict Git tar source capsule."""

import io
import tarfile
import unittest

import source_publication_records as records
import source_semver_capsule as capsule


def fixture():
    payloads = {"Cargo.lock": b"lock", "pkg/a.txt": b"a", "pkg/sub/b.txt": b"b"}
    inventory_files = {}
    for name, data in payloads.items():
        inventory_files[name] = {"sha256": capsule.sha(data),
            "git_blob": capsule.git_object("blob", data), "git_mode": "100644",
            "archive_mode": "0644", "size_bytes": len(data)}
    inventory = {"schema_version": 1, "source_commit": "a" * 40,
        "source_tree": capsule._tree_hash(inventory_files), "files": inventory_files}
    archive_data = make_archive(payloads)
    revision = records._record("semver-checker", inventory["source_commit"],
        inventory["source_tree"], "b" * 40, "example/semver",
        "GitTarUmask022V1", capsule.sha(archive_data), len(archive_data),
        "c" * 64, "d" * 64, None, "e" * 64)
    git_files = {name: (b"100644", metadata["git_blob"])
                 for name, metadata in inventory_files.items()}
    return revision, inventory, payloads, archive_data, git_files


def make_archive(payloads, extra_pax=None, global_pax=True, special=None):
    buffer = io.BytesIO()
    headers = {"comment": capsule.EXPECTED_PAX} if global_pax else {}
    with tarfile.open(fileobj=buffer, mode="w:", format=tarfile.PAX_FORMAT,
                      pax_headers=headers) as archive:
        directories = {"pkg", "pkg/sub"}
        for name in sorted(directories):
            entry = tarfile.TarInfo(name)
            entry.type, entry.mode, entry.size = tarfile.DIRTYPE, 0o755, 0
            entry.pax_headers = {"comment": capsule.EXPECTED_PAX}
            archive.addfile(entry)
        for name, data in sorted(payloads.items()):
            entry = tarfile.TarInfo(name)
            entry.mode, entry.size = 0o644, len(data)
            entry.pax_headers = {"comment": capsule.EXPECTED_PAX}
            if extra_pax and name == next(iter(payloads)):
                entry.pax_headers.update(extra_pax)
            if special and name == next(iter(payloads)):
                entry.type, entry.linkname = special
                entry.mode, entry.size = 0o777, 0
                archive.addfile(entry)
                continue
            archive.addfile(entry, io.BytesIO(data))
    return buffer.getvalue()


class SemverCapsuleTests(unittest.TestCase):
    def test_valid_git_tar_reconstructs_exact_tree_and_blobs(self):
        revision, inventory, payloads, archive_data, git_files = fixture()
        result = capsule.archive_proof(archive_data, inventory, revision, git_files)
        self.assertEqual(result, payloads)

    def test_path_and_pax_attacks_are_rejected(self):
        revision, inventory, payloads, _, git_files = fixture()
        for bad_archive in (make_archive(payloads, {"unexpected": "value"}),
                            make_archive(payloads, global_pax=False)):
            with self.assertRaises(ValueError):
                capsule.archive_proof(bad_archive, inventory, revision, git_files)
        bad_inventory = dict(inventory, files=dict(inventory["files"]))
        bad_inventory["files"]["../escape"] = bad_inventory["files"].pop("pkg/a.txt")
        with self.assertRaises(ValueError):
            capsule.archive_proof(make_archive(payloads), bad_inventory, revision, git_files)

    def test_blob_mode_tree_and_directory_closure_are_bound(self):
        revision, inventory, payloads, archive_data, git_files = fixture()
        with self.assertRaises(ValueError):
            capsule.archive_proof(archive_data, dict(inventory, schema_version=True), revision, git_files)
        changed = dict(inventory, files=dict(inventory["files"]))
        changed["files"]["Cargo.lock"] = dict(changed["files"]["Cargo.lock"], sha256="0" * 64)
        with self.assertRaises(ValueError):
            capsule.archive_proof(archive_data, changed, revision, git_files)
        changed_bool = dict(inventory, files=dict(inventory["files"]))
        changed_bool["files"]["pkg/a.txt"] = dict(changed_bool["files"]["pkg/a.txt"], size_bytes=True)
        with self.assertRaises(ValueError):
            capsule.archive_proof(archive_data, changed_bool, revision, git_files)
        with self.assertRaises(ValueError):
            capsule.archive_proof(archive_data, inventory,
                records._record("semver-checker", revision.source_commit, "0" * 40,
                    revision.upstream_base_commit, revision.upstream_repository,
                    "GitTarUmask022V1", revision.source_archive_sha256,
                    revision.source_archive_size, revision.base_patch_sha256,
                    revision.raw_commit_sha256, None, revision.source_receipt_sha256), git_files)
        missing_dir = make_archive({"Cargo.lock": payloads["Cargo.lock"], "pkg/a.txt": payloads["pkg/a.txt"]})
        with self.assertRaises(ValueError):
            capsule.archive_proof(missing_dir, inventory, revision, git_files)

    def test_portable_path_rejects_aliases_and_traversal(self):
        for name in ("", "/absolute", "../escape", "pkg/../escape", "pkg//file",
                     "pkg\\file", "pkg/./file", ".git/config", "pkg/.git/config"):
            with self.assertRaises(ValueError):
                capsule.portable_path(name)

    def test_special_entries_and_gitlinks_are_rejected(self):
        revision, inventory, payloads, _, git_files = fixture()
        for entry_type, linkname in ((tarfile.SYMTYPE, "Cargo.lock"),
                                     (tarfile.LNKTYPE, "Cargo.lock"),
                                     (tarfile.FIFOTYPE, "")):
            with self.assertRaises(ValueError):
                capsule.archive_proof(make_archive(payloads, special=(entry_type, linkname)),
                    inventory, revision, git_files)
        gitlink = dict(inventory, files=dict(inventory["files"]))
        gitlink["files"]["pkg/a.txt"] = dict(gitlink["files"]["pkg/a.txt"],
            git_mode="160000")
        with self.assertRaises(ValueError):
            capsule.archive_proof(make_archive(payloads), gitlink, revision, git_files)


if __name__ == "__main__":
    unittest.main()

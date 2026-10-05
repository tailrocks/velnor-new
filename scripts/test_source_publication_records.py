"""Closed source revision registry tests."""

from dataclasses import FrozenInstanceError
import unittest

import source_publication_records as records


TARGET = "c57c700459bbe1549fe7eedcb7d8689585c38986"
COMMITS = {
    "mise": "dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96",
    "mbx-action": "c3cbe8e56ccb4727624df45022357f49d2953075",
    "semver-checker": "583dddce84706786fc54c41a2c768c28a09c65fd",
}


class RevisionRecordTests(unittest.TestCase):
    def test_registry_contains_closed_historical_and_new_records(self):
        self.assertEqual(set(records.role_names()), set(COMMITS) | {"mbx-action"})
        self.assertEqual(len(records.REVISIONS), 4)
        for role, commit in COMMITS.items():
            revision = records.reviewed_source_revision(role, commit, TARGET)
            self.assertEqual(revision.role.value, role)
            self.assertEqual(revision.source_commit, commit)
            self.assertEqual(revision.tag_target, TARGET)
            self.assertEqual(revision.source_ref,
                "refs/heads/owned-source/" + role + "/" + commit)
            self.assertEqual(revision.tag, "owned-source-" + role + "-" + commit)

    def test_mbx_action_new_record_is_distinct_from_historical_record(self):
        old = records.reviewed_source_revision(
            "mbx-action", "c3cbe8e56ccb4727624df45022357f49d2953075", TARGET)
        new = records.reviewed_source_revision(
            "mbx-action", "62ec0713473dffeab46884b7c03906042794e696", TARGET)
        self.assertNotEqual(old.source_commit, new.source_commit)
        self.assertNotEqual(old.source_archive_sha256, new.source_archive_sha256)
        self.assertEqual(old.archive_kind, records.ArchiveKind.SOURCE_PREFIX_FILES_V1)
        self.assertEqual(new.archive_kind, records.ArchiveKind.SOURCE_PREFIX_FILES_V1)

    def test_unknown_or_rebound_authority_is_rejected(self):
        valid = records.reviewed_source_revision("mise", COMMITS["mise"], TARGET)
        for role, commit, target in (
                ("unknown", valid.source_commit, TARGET),
                ("mise", "0" * 40, TARGET),
                ("mise", valid.source_commit, "0" * 40)):
            with self.assertRaises(ValueError):
                records.reviewed_source_revision(role, commit, target)

    def test_record_is_immutable_and_has_no_public_constructor(self):
        revision = records.reviewed_source_revision("mise", COMMITS["mise"], TARGET)
        with self.assertRaises(FrozenInstanceError):
            revision.tag_target = "0" * 40
        with self.assertRaises(TypeError):
            records.ReviewedSourceRevision()


if __name__ == "__main__":
    unittest.main()

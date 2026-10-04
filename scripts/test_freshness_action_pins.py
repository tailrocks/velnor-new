"""Executable fixtures for exact immutable GitHub action commit pins."""

import sys
import unittest

sys.dont_write_bytecode = True

from freshness_action_pins import (
    commit_sha_from_response,
    github_commit_source,
    github_latest_release_sources,
    github_tag_commit_source,
    release_tag_matches,
    release_tag_sha_from_response,
    validate_action_pin,
)


SHA = "abcdef0123456789abcdef0123456789abcdef01"
OTHER_SHA = "1234567890abcdef1234567890abcdef12345678"


def commit_entry():
    return {
        "key": "jdx/mr-boxington-action",
        "pin_kind": "commit",
        "pinned_version": "commit-abcdef0",
        "pinned_sha": SHA,
        "qualified_version": "commit-abcdef0",
        "qualified_sha": SHA,
        "latest": SHA,
        "source": github_commit_source("jdx/mr-boxington-action", SHA),
    }


def body(sha):
    return '{"sha":"' + sha + '","commit":{"message":"fixture"}}'


def release_entry():
    version = "v1.7.0"
    key = "jdx/mr-boxington-action"
    return {
        "key": key,
        "pin_kind": "release",
        "pinned_version": version,
        "pinned_sha": SHA,
        "qualified_version": version,
        "qualified_sha": SHA,
        "latest": version,
        "latest_source": github_latest_release_sources(key)[0],
        "source": github_tag_commit_source(key, version),
    }


class CommitPinFixtures(unittest.TestCase):
    def test_cache_restore_and_save_use_repository_api_endpoint(self):
        expected = f"https://api.github.com/repos/actions/cache/commits/{SHA}"
        self.assertEqual(github_commit_source("actions/cache/restore", SHA), expected)
        self.assertEqual(github_commit_source("actions/cache/save", SHA), expected)

    def test_official_jdx_commit_identity_is_accepted(self):
        entry = commit_entry()
        self.assertIsNone(validate_action_pin(entry))
        self.assertTrue(entry["pinned_version"].startswith("commit-"))
        self.assertEqual(commit_sha_from_response(entry["source"], body(SHA)), SHA)

    def test_wrong_sha_is_rejected(self):
        entry = commit_entry()
        self.assertIsNone(validate_action_pin(entry))
        self.assertIsNone(commit_sha_from_response(entry["source"], body(OTHER_SHA)))

        entry["pinned_sha"] = OTHER_SHA
        self.assertIn("label must match", validate_action_pin(entry))

    def test_wrong_repository_source_is_rejected(self):
        entry = commit_entry()
        entry["source"] = github_commit_source("actions/checkout", SHA)
        self.assertIn("exact endpoint", validate_action_pin(entry))

    def test_wrong_endpoint_is_rejected(self):
        entry = commit_entry()
        entry["source"] = f"https://api.github.com/repos/actions/cache/releases/{SHA}"
        self.assertIn("exact endpoint", validate_action_pin(entry))
        self.assertIsNone(commit_sha_from_response(entry["source"], body(SHA)))

    def test_action_subpath_in_commit_endpoint_is_rejected(self):
        entry = commit_entry()
        entry["source"] = (
            f"https://api.github.com/repos/actions/cache/restore/commits/{SHA}"
        )
        self.assertIn("exact endpoint", validate_action_pin(entry))
        self.assertIsNone(commit_sha_from_response(entry["source"], body(SHA)))

    def test_action_pin_requires_explicit_mode(self):
        entry = commit_entry()
        del entry["pin_kind"]
        self.assertEqual(validate_action_pin(entry), "missing explicit pin_kind")

        entry["pinned_version"] = "v1.7.0"
        self.assertEqual(validate_action_pin(entry), "missing explicit pin_kind")

    def test_qualified_identity_must_match_commit_sha(self):
        entry = commit_entry()
        entry["qualified_sha"] = OTHER_SHA
        self.assertIn("qualified commit identity", validate_action_pin(entry))

    def test_latest_commit_evidence_must_match_sha(self):
        entry = commit_entry()
        entry["latest"] = OTHER_SHA
        self.assertEqual(
            validate_action_pin(entry),
            "commit latest evidence must match pinned_sha",
        )

    def test_release_mode_requires_semver_label(self):
        entry = commit_entry()
        entry["pin_kind"] = "release"
        self.assertEqual(
            validate_action_pin(entry),
            "release pin label must be SemVer vX.Y.Z",
        )

    def test_wrong_commit_mode_is_rejected(self):
        entry = commit_entry()
        entry["pin_kind"] = "fork-commit"
        self.assertEqual(
            validate_action_pin(entry),
            "unsupported pin_kind: 'fork-commit'",
        )

    def test_release_mode_accepts_semver_label(self):
        entry = release_entry()
        self.assertIsNone(validate_action_pin(entry))
        self.assertEqual(release_tag_sha_from_response(entry["source"], body(SHA)), SHA)
        self.assertTrue(release_tag_matches(entry, body(SHA)))

    def test_release_tag_source_binds_action_repository_and_tag(self):
        entry = release_entry()
        entry["source"] = github_tag_commit_source("actions/checkout", "v1.7.0")
        self.assertIn("exact tag endpoint", validate_action_pin(entry))

        entry = release_entry()
        entry["source"] = github_tag_commit_source(
            "jdx/mr-boxington-action", "v1.6.0",
        )
        self.assertIn("exact tag endpoint", validate_action_pin(entry))

    def test_release_tag_response_must_match_pinned_and_qualified_sha(self):
        entry = release_entry()
        self.assertFalse(release_tag_matches(entry, body(OTHER_SHA)))

        entry["qualified_sha"] = OTHER_SHA
        self.assertIn("qualified release identity", validate_action_pin(entry))
        self.assertFalse(release_tag_matches(entry, body(SHA)))

    def test_release_tag_rejects_wrong_endpoint(self):
        entry = release_entry()
        entry["source"] = entry["latest_source"]
        self.assertIn("exact tag endpoint", validate_action_pin(entry))
        self.assertFalse(release_tag_matches(entry, body(SHA)))
        self.assertIsNone(release_tag_sha_from_response(entry["source"], body(SHA)))

    def test_release_latest_endpoint_must_match_action_repository(self):
        entry = release_entry()
        entry["latest_source"] = github_latest_release_sources("actions/checkout")[0]
        self.assertEqual(
            validate_action_pin(entry),
            "release latest_source must be an exact endpoint for its repository",
        )

    def test_release_latest_source_rejects_unordered_tag_listing(self):
        entry = release_entry()
        entry["latest_source"] = (
            "https://api.github.com/repos/jdx/mr-boxington-action/tags"
        )
        self.assertEqual(
            validate_action_pin(entry),
            "release latest_source must be an exact endpoint for its repository",
        )
        self.assertNotIn(
            "https://api.github.com/repos/jdx/mr-boxington-action/tags",
            github_latest_release_sources("jdx/mr-boxington-action"),
        )


if __name__ == "__main__":
    unittest.main()

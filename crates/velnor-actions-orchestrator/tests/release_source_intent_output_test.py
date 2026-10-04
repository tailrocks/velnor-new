"""Custodied file writes resist hierarchy races; these tests grant no source/SDK."""
import hashlib
import os
import stat
import unittest
from unittest.mock import patch

import release_source_intent_context_test as context_fixture


class PreparedPayloadOutputTest(unittest.TestCase):
    def setUp(self):
        self.fixture = context_fixture.PreparedContextTest()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.ns = self.fixture.ns
        self.root = self.fixture.root
        self.output = self.fixture.output

    def context(self):
        context = self.fixture.context()
        self.addCleanup(context.output_sink.close)
        return context

    def test_exclusive_immutable_payload_and_digest_custody(self):
        context = self.context()
        raw = b"immutable original zip buffer"
        digest = context.write_prepared_payload(raw)
        self.assertEqual(digest, hashlib.sha256(raw).hexdigest())
        self.assertEqual(context.destination.read_bytes(), raw)
        self.assertEqual(stat.S_IMODE(context.destination.stat().st_mode), 0o400)
        self.assertEqual(stat.S_IMODE(context.destination.parent.stat().st_mode), 0o700)
        context.output_sink.publish_prepared_sha256(digest)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "repeated"):
            context.write_prepared_payload(b"replace")

    def test_digest_without_payload_custody_never_published(self):
        context = self.context()
        with self.assertRaisesRegex(self.ns["ReconcileError"], "digest"):
            context.output_sink.publish_prepared_sha256("a" * 64)
        self.assertEqual(self.output.read_text(), "")

    def test_different_digest_never_published(self):
        context = self.context()
        context.write_prepared_payload(b"original")
        with self.assertRaisesRegex(self.ns["ReconcileError"], "digest"):
            context.output_sink.publish_prepared_sha256("a" * 64)
        self.assertEqual(self.output.read_text(), "")

    def test_mutable_payload_rejected(self):
        context = self.context()
        with self.assertRaisesRegex(self.ns["ReconcileError"], "bytes"):
            context.write_prepared_payload(bytearray(b"mutable"))
        self.assertFalse(context.destination.exists())

    def test_preexisting_private_namespace_denied(self):
        path = self.root / "velnor/source-intent-prepared"
        path.mkdir(mode=0o700, parents=True)
        with self.assertRaises(FileExistsError):
            self.fixture.context()

    def test_symlink_parent_never_followed(self):
        outside = self.root / "outside"
        outside.mkdir(mode=0o700)
        (self.root / "velnor").symlink_to(outside, target_is_directory=True)
        with self.assertRaises(OSError):
            self.fixture.context()
        self.assertEqual(list(outside.iterdir()), [])

    def test_worker_writable_parent_denied(self):
        parent = self.root / "velnor"
        parent.mkdir(mode=0o700)
        parent.chmod(0o770)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "directory"):
            self.fixture.context()
        self.assertFalse((parent / "source-intent-prepared").exists())

    def test_rebound_parent_denied_before_payload_write(self):
        context = self.context()
        parent = self.root / "velnor"
        parent.rename(self.root / "old-private-parent")
        parent.mkdir(mode=0o700)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "rebound"):
            context.write_prepared_payload(b"original")
        self.assertFalse(context.destination.exists())
        self.assertEqual(self.output.read_text(), "")

    def test_parent_rebound_during_open_uses_owned_fd_then_denies_publication(self):
        context = self.context()
        actual_open = os.open

        def race(path, flags, *args, **kwargs):
            if path == "prepared.zip":
                parent = self.root / "velnor"
                parent.rename(self.root / "old-private-parent")
                parent.mkdir(mode=0o700)
            return actual_open(path, flags, *args, **kwargs)

        with patch.object(os, "open", side_effect=race), \
                self.assertRaisesRegex(self.ns["ReconcileError"], "rebound"):
            context.write_prepared_payload(b"original")
        self.assertFalse(context.destination.exists())
        self.assertEqual((self.root / "old-private-parent/source-intent-prepared/prepared.zip")
                         .read_bytes(), b"original")
        self.assertEqual(self.output.read_text(), "")

    def test_parent_rebound_after_write_denies_digest_publication(self):
        context = self.context()
        digest = context.write_prepared_payload(b"original")
        parent = self.root / "velnor"
        parent.rename(self.root / "old-private-parent")
        parent.mkdir(mode=0o700)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "rebound"):
            context.output_sink.publish_prepared_sha256(digest)
        self.assertEqual(self.output.read_text(), "")

    def test_payload_modified_with_restored_mode_denies_publication(self):
        context = self.context()
        digest = context.write_prepared_payload(b"original")
        context.destination.chmod(0o600)
        context.destination.write_bytes(b"modified")
        context.destination.chmod(0o400)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "payload_changed"):
            context.output_sink.publish_prepared_sha256(digest)
        self.assertEqual(self.output.read_text(), "")

    def test_payload_replaced_with_equal_bytes_denies_publication(self):
        context = self.context()
        digest = context.write_prepared_payload(b"original")
        context.destination.rename(context.destination.with_suffix(".old"))
        context.destination.write_bytes(b"original")
        context.destination.chmod(0o400)
        with self.assertRaisesRegex(self.ns["ReconcileError"], "payload_(changed|rebound)"):
            context.output_sink.publish_prepared_sha256(digest)
        self.assertEqual(self.output.read_text(), "")


if __name__ == "__main__":
    unittest.main()

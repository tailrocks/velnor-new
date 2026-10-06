"""Real owned executable size admission with bounded streaming memory."""
import hashlib
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import catalog_executable_bounds as bounds
import catalog_native_health as health

OWNED_BINARY_BYTES = 153367552


def zero_digest(size):
    digest = hashlib.sha256()
    block = bytes(1024 * 1024)
    while size:
        count = min(size, len(block))
        digest.update(block[:count])
        size -= count
    return digest.hexdigest()


class TrackingStream(io.BytesIO):
    def __init__(self, contents):
        super().__init__(contents)
        self.returned = 0
        self.requests = []

    def read(self, size=-1):
        self.requests.append(size)
        result = super().read(size)
        self.returned += len(result)
        return result


class ExecutableBoundsTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.binary = self.root / "mise"
        for name in ("executable_limit", "archive_limit", "stream_sha256"):
            binding = patch.object(health, name, getattr(bounds, name), create=True)
            binding.start()
            self.addCleanup(binding.stop)

    def sparse_binary(self, size):
        with self.binary.open("wb") as output:
            output.truncate(size)
        self.binary.chmod(0o700)

    def test_distribution_size_streams_and_native_manager_admits(self):
        self.sparse_binary(OWNED_BINARY_BYTES)
        expected = zero_digest(OWNED_BINARY_BYTES)
        with self.binary.open("rb") as source:
            self.assertEqual(bounds.stream_sha256(source), expected)
        directory = os.open(self.root, os.O_RDONLY | os.O_DIRECTORY)
        try:
            health.verify_manager(directory, expected)
        finally:
            os.close(directory)

    def test_limit_identity(self):
        self.assertEqual(bounds.executable_limit(), 256 * 1024 * 1024)
        self.assertEqual(bounds.archive_limit(), bounds.executable_limit() + 32 * 1024 * 1024)

    def test_stream_cap_plus_one_rejects_with_bounded_reads(self):
        source = TrackingStream(b"x" * 32)
        with patch.object(bounds, "executable_limit", return_value=8):
            with self.assertRaises(ValueError):
                bounds.stream_sha256(source)
        self.assertLessEqual(source.returned, 9)
        self.assertTrue(source.requests)
        self.assertTrue(all(0 <= size <= 9 for size in source.requests))

    def test_stream_exact_cap_admits(self):
        source = TrackingStream(b"x" * 8)
        with patch.object(bounds, "executable_limit", return_value=8):
            self.assertEqual(bounds.stream_sha256(source), hashlib.sha256(b"x" * 8).hexdigest())

    def test_oversize_native_manager_rejects_before_read(self):
        self.sparse_binary(bounds.executable_limit() + 1)
        directory = os.open(self.root, os.O_RDONLY | os.O_DIRECTORY)
        try:
            with patch.object(health, "stream_sha256", side_effect=AssertionError("oversize read")):
                with self.assertRaisesRegex(ValueError, "manager_shape"):
                    health.verify_manager(directory, "0" * 64)
        finally:
            os.close(directory)


if __name__ == "__main__":
    unittest.main()

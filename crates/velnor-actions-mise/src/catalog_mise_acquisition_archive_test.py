"""Security cases for the Mise archive's closed member contract."""

import gzip
import hashlib
import io
import tarfile
import unittest
from unittest.mock import patch

import catalog_mise_acquisition_archive as acquisition
from catalog_executable_bounds import executable_limit, archive_limit, stream_sha256
acquisition.executable_limit = executable_limit
acquisition.archive_limit = archive_limit
acquisition.stream_sha256 = stream_sha256


BINARY = b"authenticated mise binary"
DIGEST = hashlib.sha256(BINARY).hexdigest()
MEMBER = "mise/bin/mise"


def archive(entries, pax=True):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as tar:
        for name, contents, kind in entries:
            member = tarfile.TarInfo(name)
            if pax:
                member.pax_headers = {"path": name}
            member.type = kind
            member.size = len(contents) if kind == tarfile.REGTYPE else 0
            if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                member.linkname = MEMBER
            tar.addfile(member, io.BytesIO(contents))
    return output.getvalue()


def regular(name=MEMBER, contents=BINARY):
    return (name, contents, tarfile.REGTYPE)


class ArchiveSecurityTests(unittest.TestCase):
    def extract(self, contents, digest=DIGEST, member=MEMBER):
        return acquisition.extract_binary(contents, digest, member)

    def test_source_qualified_binary(self):
        entries = [("mise", b"", tarfile.DIRTYPE), regular(), regular("LICENSE", b"license")]
        self.assertEqual(self.extract(archive(entries)), BINARY)
        self.assertEqual(self.extract(archive([regular("bin/mise")]), member="bin/mise"), BINARY)

    def test_unsafe_regular_paths(self):
        paths = ["/mise", "../mise", "a/../mise", "./mise", "a/./mise", "a//mise",
                 "mise/", "a\\mise", "a\x00mise", "a\nmise", "a\x7fmise", "C:/mise",
                 "a\u202emise", "x" * 256]
        for name in paths:
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.extract(archive([regular(), regular(name)]))

    def test_expected_path_must_be_safe(self):
        for name in ("", "/mise", "../mise", "a//mise", "a\\mise"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                self.extract(archive([regular()]), member=name)

    def test_raw_and_pax_terminal_slashes(self):
        for pax in (False, True):
            for kind in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE):
                name = "other//" if kind == tarfile.DIRTYPE else "other/"
                with self.subTest(pax=pax, kind=kind), self.assertRaises(ValueError):
                    self.extract(archive([regular(), (name, b"", kind)], pax=pax))

    def test_single_directory_terminal_slash(self):
        for pax in (False, True):
            entries = [("mise/", b"", tarfile.DIRTYPE), regular()]
            with self.subTest(pax=pax):
                self.assertEqual(self.extract(archive(entries, pax=pax)), BINARY)

    def test_links_and_special_members(self):
        kinds = [tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE, tarfile.BLKTYPE,
                 tarfile.FIFOTYPE, tarfile.GNUTYPE_SPARSE, b"Z"]
        for kind in kinds:
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                self.extract(archive([regular(), ("other", b"", kind)]))

    def test_duplicate_any_member(self):
        for entries in ([regular(), regular()],
                        [regular(), regular("README", b"a"), regular("README", b"b")]):
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                self.extract(archive(entries))

    def test_pax_sparse_metadata(self):
        for fields in ({"GNU.sparse.map": "0,1"}, {"GNU.sparse.size": "1"},
                       {"GNU.sparse.major": "1", "GNU.sparse.minor": "0"},
                       {"GNU.sparse.name": MEMBER}):
            output = io.BytesIO()
            with tarfile.open(fileobj=output, mode="w:gz") as tar:
                member = tarfile.TarInfo(MEMBER)
                member.pax_headers = fields
                member.size = len(BINARY)
                tar.addfile(member, io.BytesIO(BINARY))
            with self.subTest(fields=fields), self.assertRaisesRegex(ValueError, "sparse"):
                self.extract(output.getvalue())

    def test_missing_and_directory_binary(self):
        for entries in ([regular("other")], [(MEMBER, b"", tarfile.DIRTYPE)]):
            with self.subTest(entries=entries), self.assertRaises(ValueError):
                self.extract(archive(entries))

    def test_binary_hash_and_digest_format(self):
        for digest in ("0" * 64, "A" * 64, "bad", "", None):
            with self.subTest(digest=digest), self.assertRaises(ValueError):
                self.extract(archive([regular()]), digest=digest)

    def test_declared_regular_bytes_limit(self):
        contents = archive([regular(), regular("padding", b"extra")])
        with patch.object(acquisition, "executable_limit", return_value=len(BINARY)):
            with self.assertRaisesRegex(ValueError, "regular bytes"):
                self.extract(contents)

    def test_member_count_limit(self):
        contents = archive([regular(), regular("extra")])
        with patch.object(acquisition, "MAX_MEMBERS", 1):
            with self.assertRaisesRegex(ValueError, "member count"):
                self.extract(contents)

    def test_decompressed_archive_limit(self):
        contents = gzip.compress(b"\0" * 4096)
        with patch.object(acquisition, "archive_limit", return_value=1024):
            with self.assertRaisesRegex(ValueError, "decompressed archive"):
                self.extract(contents)

    def test_path_length_limit(self):
        name = "/".join(["a" * 200] * 21)
        with self.assertRaisesRegex(ValueError, "path exceeds"):
            self.extract(archive([regular(), regular(name)]))

    def test_malformed_and_truncated_archives(self):
        for contents in (b"not gzip", gzip.compress(b"not tar"), archive([regular()])[:20]):
            with self.subTest(contents=contents), self.assertRaises(ValueError):
                self.extract(contents)


if __name__ == "__main__":
    unittest.main()

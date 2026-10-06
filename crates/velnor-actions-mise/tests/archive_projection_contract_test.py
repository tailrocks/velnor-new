"""Independent framing fixture and source closure checks; does NOT execute Rust."""
import hashlib
from pathlib import Path
import re
import struct
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "src"


def frame(value):
    return struct.pack(">Q", len(value)) + value


def identity(purpose, fields):
    value = frame(b"velnor-source-archive-projection-v1") + frame(purpose)
    value += struct.pack(">Q", len(fields))
    value += b"".join(frame(field) for field in fields)
    return hashlib.sha256(value).hexdigest()


class IndependentFramingFixture(unittest.TestCase):
    def test_recorded_fixture_matches_actual_proposed_rust_source(self):
        expected = identity(b"capability-v1", [b"ab", b"c"])
        self.assertEqual(expected,
                         "6c6ec732e88f1b562fd70ab2c0daf9abfc51c08e082ec8af581bb572985b2e05")
        self.assertIn(expected, (SOURCE / "archive_projection_tests.rs").read_text())
        rust = (SOURCE / "archive_projection_identity.rs").read_text()
        self.assertIn('frame(&mut bytes, b"velnor-source-archive-projection-v1")', rust)
        self.assertIn('frame(&mut bytes, purpose.as_bytes())', rust)
        self.assertIn('(fields.len() as u64).to_be_bytes()', rust)
        self.assertIn('(value.len() as u64).to_be_bytes()', rust)

    def test_length_order_count_and_purpose_separation(self):
        baseline = identity(b"capability-v1", [b"ab", b"c"])
        for purpose, fields in [
            (b"capability-v1", [b"a", b"bc"]),
            (b"capability-v1", [b"c", b"ab"]),
            (b"capability-v1", [b"ab", b"c", b""]),
            (b"payload-v1", [b"ab", b"c"]),
        ]:
            self.assertNotEqual(baseline, identity(purpose, fields))

    def test_no_production_constructor_or_activation(self):
        source = (SOURCE / "archive_projection.rs").read_text()
        self.assertRegex(source, r"qualified_source_archive_projection\(\).*?\{\s*None\s*\}")
        self.assertRegex(source, r"#\[cfg\(test\)\]\s*fn fixture\(")
        self.assertNotRegex(source, r"pub(?:\([^)]*\))? fn (?:new|fixture|from_)")
        self.assertNotIn("Deserialize", source)
        self.assertNotIn("std::env", source)
        self.assertIn('framed_sha256("payload-v1", &fields)', source)
        self.assertNotIn("CompiledSourceHelper::compiled", source)

    def test_tuple_binds_all_fourteen_roles(self):
        source = (SOURCE / "archive_projection_identity.rs").read_text()
        array = re.search(r"let fields = \[(.*?)\];", source, re.S).group(1)
        fields = re.findall(r"self\.([a-z_.0-9]+)\.as_bytes\(\)", array)
        self.assertEqual(len(fields), 14)
        self.assertEqual(len(set(fields)), 14)
        self.assertIn("inventory_template_sha256", fields)
        self.assertIn("runtime.closure_sha256", fields)
        self.assertNotIn("final_helper_sha256", fields)


if __name__ == "__main__":
    unittest.main()

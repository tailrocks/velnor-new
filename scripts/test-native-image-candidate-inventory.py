# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "native_inventory", Path(__file__).with_name("native-image-candidate-inventory.py")
)
inventory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inventory)


class DepInfoTests(unittest.TestCase):
    def test_first_rule_preserves_escaped_paths_and_ignores_later_records(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "artifact.d"
            for newline in ("\n", "\r\n"):
                with self.subTest(newline=repr(newline)):
                    contents = (
                        r"target\:name: src/with\ space.rs path\\name.rs " + "\\"
                        + newline
                        + " nested/other.rs"
                        + newline
                        + newline
                        + "phony: injected.rs"
                        + newline
                        + "# env-dep:VELNOR_SOURCE_SHA=ignored"
                        + newline
                    )
                    path.write_bytes(contents.encode())
                    self.assertEqual(
                        inventory.make_prerequisites(path),
                        ["src/with space.rs", r"path\name.rs", "nested/other.rs"],
                    )



if __name__ == "__main__":
    unittest.main()

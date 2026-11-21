# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import importlib.util
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("pseudonym-consistency.py")
spec = importlib.util.spec_from_file_location("pseudonym_consistency", SCRIPT)
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

SHA = "6b2ac2277103a22b9ae155bc92415ca5ddf05f95"
OTHER = "0881638712a837bdcb90b3cd811a8a9be3aa8d83"


def repository(files):
    tmp = tempfile.TemporaryDirectory(prefix="pseudonym-test-")
    root = Path(tmp.name)
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    subprocess.run(["git", "init", "-q", str(root)], check=True, env=env)
    for name, text in files.items():
        (root / name).write_text(text)
    subprocess.run(["git", "-C", str(root), "add", "-A"], check=True, env=env)
    return tmp, root


class ConsistencyTests(unittest.TestCase):
    def test_same_record_and_sha_with_one_pseudonym_everywhere_passes(self):
        tmp, root = repository(
            {
                "scope.csv": f"44,C,example-org/example-repo-9,{SHA},x\n",
                "scope.md": f"| 44 | C | `example-org/example-repo-9` | {SHA} |\n",
                "scope.json": f'  {{"number": 44, "repository": "example-org/example-repo-9", "sha": "{SHA}"}}\n',
            }
        )
        with tmp:
            self.assertEqual(check.disagreements(root), [])

    def test_same_record_with_different_pseudonyms_in_two_files_fails(self):
        tmp, root = repository(
            {
                "scope.csv": f"44,C,example-org/example-repo-9,{SHA},x\n",
                "scope.md": f"| 44 | C | `example-org/example-repo-3` | {SHA} |\n",
            }
        )
        with tmp:
            found = check.disagreements(root)
            self.assertEqual([label for label, _names in found], [b"record:44"])
            self.assertEqual(sorted(found[0][1]), [b"example-repo-3", b"example-repo-9"])

    def test_same_repository_node_id_with_different_pseudonyms_fails(self):
        tmp, root = repository(
            {
                "a.csv": "scope,example-org/example-repo-1,R_kgDOR3ckuA,true\n",
                "b.csv": "scope,example-org/example-repo-2,R_kgDOR3ckuA,true\n",
            }
        )
        with tmp:
            self.assertEqual([label for label, _names in check.disagreements(root)], [b"node:R_kgDOR3ckuA"])

    def test_different_records_and_unkeyed_prose_may_use_different_pseudonyms(self):
        tmp, root = repository(
            {
                "a.csv": f"44,C,example-org/example-repo-9,{SHA},x\n",
                "b.csv": f"45,C,example-org/example-repo-3,{OTHER},x\n",
                "c.md": f"The tag points at {SHA} in example-org/example-repo-5.\n",
                "d.md": f"| 44 | C | `example-org/example-repo-9` and `example-org/example-repo-4` | {SHA} |\n",
            }
        )
        with tmp:
            self.assertEqual(check.disagreements(root), [])


if __name__ == "__main__":
    unittest.main()

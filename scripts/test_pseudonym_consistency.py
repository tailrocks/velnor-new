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


def repo(number):
    return f"example-repo-{number}"


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
                "scope.csv": f"44,C,example-org/{repo(9)},{SHA},x\n",
                "scope.md": f"| 44 | C | `example-org/{repo(9)}` | {SHA} |\n",
                "scope.json": f'  {{"number": 44, "repository": "example-org/{repo(9)}", "sha": "{SHA}"}}\n',
            }
        )
        with tmp:
            self.assertEqual(check.disagreements(root), [])

    def test_same_record_with_different_pseudonyms_in_two_files_fails(self):
        tmp, root = repository(
            {
                "scope.csv": f"44,C,example-org/{repo(9)},{SHA},x\n",
                "scope.md": f"| 44 | C | `example-org/{repo(3)}` | {SHA} |\n",
            }
        )
        with tmp:
            found = check.disagreements(root)
            self.assertEqual([label for label, _names in found], [b"record:44"])
            self.assertEqual(sorted(found[0][1]), [repo(3).encode(), repo(9).encode()])

    def test_same_repository_node_id_with_different_pseudonyms_fails(self):
        tmp, root = repository(
            {
                "a.csv": f"scope,example-org/{repo(1)},R_kgDOR3ckuA,true\n",
                "b.csv": f"scope,example-org/{repo(2)},R_kgDOR3ckuA,true\n",
            }
        )
        with tmp:
            self.assertEqual([label for label, _names in check.disagreements(root)], [b"node:R_kgDOR3ckuA"])

    def test_different_records_and_unkeyed_prose_may_use_different_pseudonyms(self):
        tmp, root = repository(
            {
                "a.csv": f"44,C,example-org/{repo(9)},{SHA},x\n",
                "b.csv": f"45,C,example-org/{repo(3)},{OTHER},x\n",
                "c.md": f"The tag points at {SHA} in example-org/{repo(5)}.\n",
                "d.md": f"| 44 | C | `example-org/{repo(9)}` and `example-org/{repo(4)}` | {SHA} |\n",
            }
        )
        with tmp:
            self.assertEqual(check.disagreements(root), [])

    def test_row_with_repository_and_owner_is_compared_by_its_repository(self):
        tmp, root = repository(
            {
                "scope.json": f'{{"number": 44, "repository": "example-org/{repo(9)}", "iac_owner": "example-org/{repo(5)}", "sha": "{SHA}"}}\n',
                "scope.md": f"| 44 | C | `example-org/{repo(3)}` | {SHA} |\n",
            }
        )
        with tmp:
            found = check.disagreements(root)
            self.assertEqual([label for label, _names in found], [b"record:44"])
            self.assertEqual(sorted(found[0][1]), [repo(3).encode(), repo(9).encode()])

    def test_owner_pseudonym_after_the_repository_does_not_conflict(self):
        tmp, root = repository(
            {
                "scope.json": f'{{"number": 44, "repository": "example-org/{repo(9)}", "iac_owner": "example-org/{repo(5)}", "sha": "{SHA}"}}\n',
                "scope.csv": f"44,C,example-org/{repo(9)},{SHA},x\n",
            }
        )
        with tmp:
            self.assertEqual(check.disagreements(root), [])


class OwnerFieldTests(unittest.TestCase):
    def test_unaliased_owner_in_iac_owner_fails(self):
        tmp, root = repository(
            {
                "readiness.json": '{"repositories": [{"number": 1, "iac_owner": "acme-labs/widget-infra"}]}\n',
            }
        )
        with tmp:
            found = check.unscrubbed_owners(root)
            self.assertEqual(len(found), 1)
            self.assertEqual(found[0][0], "readiness.json")

    def test_aliased_and_project_owners_in_iac_owner_pass(self):
        tmp, root = repository(
            {
                "readiness.json": (
                    '{"repositories": ['
                    '{"number": 1, "iac_owner": "tailrocks/github-terraform"}, '
                    '{"number": 2, "iac_owner": "example-org/example-repo-5"}, '
                    '{"number": 3, "iac_owner": "example-org-b/github-terraform"}]}\n'
                ),
            }
        )
        with tmp:
            self.assertEqual(check.unscrubbed_owners(root), [])


if __name__ == "__main__":
    unittest.main()

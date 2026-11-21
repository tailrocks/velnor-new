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
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(text)
    subprocess.run(["git", "-C", str(root), "add", "-A"], check=True, env=env)
    return tmp, root


def indexed(entries):
    tmp = tempfile.TemporaryDirectory(prefix="pseudonym-test-")
    root = Path(tmp.name)
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    subprocess.run(["git", "init", "-q", str(root)], check=True, env=env)
    for path, data in entries.items():
        obj = subprocess.run(
            ["git", "-C", str(root), "hash-object", "-w", "--stdin"], input=data, check=True, capture_output=True, env=env
        ).stdout.decode().strip()
        subprocess.run(
            ["git", "-C", str(root), "update-index", "--add", "--cacheinfo", f"100644,{obj},".encode() + path], check=True, env=env
        )
    return tmp, root


def pretty(number, name, sha=SHA):
    return f'[\n  {{\n    "number": {number},\n    "wave": "C",\n    "repository": "example-org/{name}",\n    "sha": "{sha}"\n  }}\n]\n'


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


class MultiLineJsonTests(unittest.TestCase):
    def test_pretty_printed_record_disagreeing_with_a_companion_row_fails(self):
        tmp, root = repository(
            {"docs/reviews/a.json": pretty(44, repo(9)), "docs/reviews/b.md": f"| 44 | C | `example-org/{repo(3)}` | {SHA} |\n"}
        )
        with tmp:
            found = check.disagreements(root)
            self.assertEqual([label for label, _names in found], [b"record:44"])
            self.assertEqual(sorted(found[0][1]), [repo(3).encode(), repo(9).encode()])

    def test_pretty_printed_record_agreeing_with_a_companion_row_passes(self):
        tmp, root = repository(
            {"a.json": pretty(44, repo(9)), "b.md": f"| 44 | C | `example-org/{repo(9)}` | {SHA} |\n", "c.json": pretty(45, repo(2), OTHER)}
        )
        with tmp:
            self.assertEqual(check.disagreements(root), [])

    def test_two_pretty_printed_files_disagreeing_on_one_node_id_fail(self):
        node = '[\n  {\n    "repository": "example-org/%s",\n    "node_id": "R_kgDOR3ckuA"\n  }\n]\n'
        tmp, root = repository({"a.json": node % repo(1), "b.json": node % repo(2)})
        with tmp:
            self.assertEqual([label for label, _names in check.disagreements(root)], [b"node:R_kgDOR3ckuA"])

    def test_string_record_numbers_and_listed_repositories_are_compared(self):
        tmp, root = repository(
            {
                "a.json": f'{{"number": "44", "repositories": ["example-org/{repo(9)}"], "sha": "{SHA}"}}\n',
                "b.md": f"| 44 | C | `example-org/{repo(3)}` | {SHA} |\n",
            }
        )
        with tmp:
            self.assertEqual([label for label, _names in check.disagreements(root)], [b"record:44"])

    def test_json_that_cannot_be_parsed_is_reported_wherever_it_names_a_pseudonym(self):
        tmp, root = indexed(
            {
                b"anywhere/a.json": f'{{"repository": "example-org/{repo(1)}",'.encode(),
                b"anywhere/b.json": f'{{"repository": "example-org/{repo(1)}"}}\xff'.encode(),
                b"anywhere/c.json": b"[" * 200000 + f"example-org/{repo(1)}".encode(),
                b"anywhere/d.json": b'{"no": "pseudonym"',
                b"anywhere/e.json": f'{{"repository": "example-org/{repo(1)}"}}'.encode(),
            }
        )
        with tmp:
            self.assertEqual(check.broken_json(root), ["anywhere/a.json", "anywhere/b.json", "anywhere/c.json"])


class OwnerFieldTests(unittest.TestCase):
    def found(self, files):
        tmp, root = repository(files)
        with tmp:
            return check.unscrubbed_owners(root)

    def test_unaliased_owner_in_iac_owner_fails(self):
        found = self.found(
            {"docs/reviews/readiness.json": '{"repositories": [{"number": 1, "iac_owner": "acme-labs/widget-infra"}]}\n'}
        )
        self.assertEqual(found, [("docs/reviews/readiness.json", 1, "iac_owner")])

    def test_every_owner_account_login_and_org_field_is_checked(self):
        for key in ["owner", "account", "login", "org", "organization", "iac_owner", "github_account", "pr_login"]:
            with self.subTest(key=key):
                found = self.found({"docs/proposed/x.json": f'{{"number": 7, "{key}": "acme-labs"}}\n'})
                self.assertEqual(found, [("docs/proposed/x.json", 7, key)])

    def test_nested_and_listed_fields_are_checked(self):
        found = self.found(
            {"docs/reviews/x.json": '{"a": [{"b": {"number": 3, "account": "acme-labs"}}, {"b": {"login": "acme-two"}}]}\n'}
        )
        self.assertEqual(sorted(key for _path, _number, key in found), ["account", "login"])

    def test_aliased_and_project_owners_pass(self):
        found = self.found(
            {
                "docs/reviews/readiness.json": (
                    '{"repositories": ['
                    '{"number": 1, "iac_owner": "tailrocks/github-terraform"}, '
                    '{"number": 2, "iac_owner": "example-org/example-repo-5"}, '
                    '{"number": 4, "account": "example-account-1"}, '
                    '{"number": 3, "iac_owner": "example-org-b/github-terraform"}]}\n'
                ),
            }
        )
        self.assertEqual(found, [])

    def test_role_descriptions_and_task_ids_are_not_accounts(self):
        found = self.found(
            {
                "docs/proposed/x.json": (
                    '{"owner": "Generator (pinned release); consumers own inputs", '
                    '"owner_task": "T58", "ownership_contract": "x own y", "enforced_by_owner": false}\n'
                ),
            }
        )
        self.assertEqual(found, [])

    def test_json_outside_the_evidence_directories_is_not_checked(self):
        self.assertEqual(self.found({"docs/implemented/api.json": '{"login": "acme-labs"}\n'}), [])

    def test_csv_columns_named_like_owner_fields_are_checked(self):
        found = self.found({"docs/reviews/rows.csv": "number,owner,note\n5,acme-labs,x\n6,example-org,y\n"})
        self.assertEqual(found, [("docs/reviews/rows.csv", None, "owner")])


class FailClosedOwnerTests(unittest.TestCase):
    def found(self, entries):
        tmp, root = indexed(entries)
        with tmp:
            return check.unscrubbed_owners(root)

    def test_non_utf8_evidence_path_is_checked_not_a_crash(self):
        found = self.found({b"docs/reviews/caf\xe9.json": b'{"number": 2, "login": "acme-labs"}\n'})
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0][1:], (2, "login"))
        self.assertEqual(check.describe(found[0][0]), "docs/reviews/caf\\xe9.json")

    def test_malformed_evidence_json_is_reported_not_clean(self):
        found = self.found({b"docs/reviews/x.json": b'{"number": 1, "login": "acme-labs"'})
        self.assertEqual(found, [("docs/reviews/x.json", None, "unparseable")])

    def test_evidence_json_with_invalid_utf8_is_reported_not_clean(self):
        found = self.found({b"docs/proposed/x.json": b'{"login": "acme-labs\xff"}\n'})
        self.assertEqual(found, [("docs/proposed/x.json", None, "unparseable")])

    def test_too_deeply_nested_evidence_json_is_reported_not_clean(self):
        found = self.found({b"docs/reviews/x.json": b"[" * 200000})
        self.assertEqual(found, [("docs/reviews/x.json", None, "unparseable")])

    def test_evidence_csv_with_invalid_utf8_or_nul_is_reported_not_clean(self):
        found = self.found(
            {b"docs/reviews/a.csv": b"number,owner\n1,acme-labs\xff\n", b"docs/reviews/b.csv": b"number,owner\n1,acme\x00labs\n"}
        )
        self.assertEqual(
            found, [("docs/reviews/a.csv", None, "unparseable"), ("docs/reviews/b.csv", None, "unparseable")]
        )

    def test_evidence_present_only_in_the_index_is_read_from_the_index(self):
        found = self.found({b"docs/reviews/x.json": b'{"number": 4, "owner": "acme-labs"}\n'})
        self.assertEqual(found, [("docs/reviews/x.json", 4, "owner")])


BOM = b"\xef\xbb\xbf"


class StructuralFailClosedTests(unittest.TestCase):
    def found(self, name, data):
        tmp, root = indexed({name: data})
        with tmp:
            return check.unscrubbed_owners(root)

    def test_every_shape_that_can_carry_an_unaliased_owner_is_reported(self):
        unparseable = [(None, "unparseable")]
        cases = {
            "list-valued owner": ("x.json", b'{"number": 1, "owner": ["example-org", "acme-labs"]}', [(1, "owner")]),
            "list-valued plural key": ("x.json", b'{"number": 2, "owners": ["acme-labs"]}', [(2, "owners")]),
            "nested object under owner": ("x.json", b'{"number": 3, "owner": {"name": "acme-labs"}}', [(3, "owner")]),
            "list of objects under owner": ("x.json", b'{"number": 4, "account": [{"id": {"v": "acme-labs"}}]}', [(4, "account")]),
            "number-typed owner": ("x.json", b'{"number": 5, "owner": 12345}', [(5, "owner")]),
            "float-typed owner in a list": ("x.json", b'{"number": 6, "login": [1.5]}', [(6, "login")]),
            "upper-case key": ("x.json", b'{"number": 7, "OWNER": "acme-labs"}', [(7, "OWNER")]),
            "mixed-case key": ("x.json", b'{"number": 8, "Iac_Owner": "acme-labs"}', [(8, "Iac_Owner")]),
            "camel-case key": ("x.json", b'{"number": 9, "iacOwner": "acme-labs"}', [(9, "iacOwner")]),
            "hyphenated key": ("x.json", b'{"number": 10, "github-account": "acme-labs"}', [(10, "github-account")]),
            "BOM before JSON": ("x.json", BOM + b'{"number": 11, "owner": "acme-labs"}', [(11, "owner")]),
            "duplicate JSON key": ("x.json", b'{"number": 12, "owner": "acme-labs", "owner": "example-org"}', unparseable),
            "duplicate JSON key in nested object": ("x.json", b'{"a": {"owner": "example-org", "owner": "acme-labs"}}', unparseable),
            "BOM before CSV header": ("x.csv", BOM + b"owner,note\nacme-labs,x\n", [(None, "owner")]),
            "upper-case CSV header": ("x.csv", b"number,OWNER\n1,acme-labs\n", [(None, "OWNER")]),
            "list-valued CSV cell": ("x.csv", b"number,owners\n1,example-org; acme-labs\n", [(None, "owners")]),
            "duplicate CSV owner header": ("x.csv", b"owner,owner\nacme-labs,example-org\n", [(None, "owner")]),
            "BOM before TSV header": ("x.tsv", BOM + b"owner\tnote\nacme-labs\tx\n", [(None, "owner")]),
            "upper-case extension": ("x.JSON", b'{"number": 13, "owner": "acme-labs"}', [(13, "owner")]),
        }
        for label, (name, data, expected) in cases.items():
            with self.subTest(label):
                path = f"docs/reviews/{name}"
                found = self.found(path.encode(), data)
                self.assertEqual(found, [(path, number, key) for number, key in expected])

    def test_aliased_owners_pass_in_every_shape_the_scan_walks(self):
        cases = {
            "x.json": BOM + b'{"number": 1, "Owner": ["example-org", {"n": "tailrocks/github-terraform"}], "login": null, "owner_ok": true}',
            "y.csv": BOM + b"Owner,note\nexample-org; example-account-1,x\n",
            "z.tsv": b"owner\tnote\nexample-org\tx\n",
        }
        for name, data in cases.items():
            with self.subTest(name):
                self.assertEqual(self.found(f"docs/proposed/{name}".encode(), data), [])

    def test_a_value_of_a_type_the_check_does_not_understand_is_not_acceptable(self):
        for value in [1, 1.5, object(), b"acme-labs", ("acme-labs",), {"acme-labs"}]:
            with self.subTest(value=value):
                self.assertFalse(check.acceptable(value))
        for value in [None, True, False, "example-org", "tailrocks/x", "a free text description"]:
            with self.subTest(value=value):
                self.assertTrue(check.acceptable(value))

    def test_duplicate_keys_and_a_leading_BOM_are_handled_the_same_way_for_pseudonym_records(self):
        tmp, root = indexed(
            {
                b"a.json": BOM + f'{{"number": 44, "repository": "example-org/{repo(9)}", "sha": "{SHA}"}}'.encode(),
                b"b.json": f'{{"repository": "example-org/{repo(9)}", "repository": "example-org/{repo(1)}"}}'.encode(),
            }
        )
        with tmp:
            self.assertEqual(check.broken_json(root), ["b.json"])
            self.assertEqual(check.disagreements(root), [])


if __name__ == "__main__":
    unittest.main()

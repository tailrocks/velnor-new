"""Source release transaction mocks; no remote writes, signing, or source execution."""

import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import publish_owned_source as P
import source_publication as S
import source_publication_records as records
import source_release_policy as R
from test_source_publication import TARGET, fixture


class PolicyTests(unittest.TestCase):
    def test_repository_api_has_no_trailing_slash(self):
        with patch.object(P, "gh", return_value=b'{"default_branch":"main"}') as command:
            self.assertEqual(P.api(""), {"default_branch": "main"})
        command.assert_called_once_with("api", "repos/tailrocks/velnor-new")

    def test_source_or_owned_tool_latest_is_rejected(self):
        for tag in ("owned-source-mise-" + "a" * 40, "mise-v1.2.3-owned-test", "mbx-v1.2.3-owned-test"):
            with patch.object(P, "api", return_value={"id": 1, "tag_name": tag}), self.assertRaises(ValueError):
                P.generator_latest()

    def test_reviewed_constraints_require_live_exact_values(self):
        for expected in (R.MAIN, R.TAGS):
            R.reviewed_ruleset(copy.deepcopy(expected), expected)
            mutations = [lambda value: value.update(enforcement="disabled"),
                lambda value: value.update(current_user_can_bypass="always"),
                lambda value: value.update(bypass_actors=[{"actor_id": 1}]),
                lambda value: value["conditions"]["ref_name"].update(exclude=["~ALL"]),
                lambda value: value["rules"].pop(), lambda value: value.update(id=True)]
            for mutation in mutations:
                actual = copy.deepcopy(expected)
                mutation(actual)
                with self.assertRaises(ValueError):
                    R.reviewed_ruleset(actual, expected)


class FakeGithub:
    def __init__(self, test, approved, role, fault, recovery):
        self.test, self.approved, self.role, self.fault, self.recovery = test, approved, role, fault, recovery
        self.commands, self.uploaded = [], {}
        self.state = {"tag": recovery, "release": recovery, "draft": True}
        self.source_ref = "refs/heads/owned-source/" + role + "/" + approved["commit"]
        self.tag = "owned-source-" + role + "-" + approved["commit"]
        self.release_id = P.RECOVERY_ID if recovery else 42

    def release(self):
        return {"id": self.release_id, "tag_name": self.tag, "target_commitish": TARGET,
            "draft": self.state["draft"], "prerelease": False, "immutable": self.fault != "final_immutable",
            "assets": [{"name": name, "id": P.RECOVERY_ASSETS[name] if self.recovery else i + 1,
                        "digest": "sha256:" + P.sha(data)}
                       for i, (name, data) in enumerate(self.uploaded.items())]}

    def __call__(self, *arguments):
        self.commands.append(arguments)
        if arguments[:3] == ("api", "--method", "POST"):
            if arguments[3] == P.PREFIX + "releases":
                body = json.loads(Path(arguments[arguments.index("--input") + 1]).read_text())
                self.test.assertTrue(body["draft"])
                self.test.assertEqual(body["make_latest"], "false")
                self.test.assertEqual(body["target_commitish"], TARGET)
                self.test.assertEqual(body["tag_name"], self.tag)
                self.state["release"] = True
                return json.dumps(self.release()).encode()
            if self.fault == "tag_race":
                raise subprocess.CalledProcessError(1, arguments, stderr=b"gh: conflict (HTTP 422)")
            self.state["tag"] = True
            return b"{}"
        if arguments[:3] == ("api", "--method", "PATCH"):
            self.test.assertEqual(arguments[arguments.index("--raw-field") + 1], "make_latest=false")
            self.state["draft"] = False
            return b"{}"
        if arguments[:2] == ("release", "upload"):
            self.uploaded[Path(arguments[3]).name] = Path(arguments[3]).read_bytes()
            return b"{}"
        self.test.assertEqual(arguments[0], "api")
        path = "" if arguments[1] == P.PREFIX.rstrip("/") else arguments[1].removeprefix(P.PREFIX)
        if path == "":
            result = {"default_branch": "main"}
        elif path.startswith("rulesets/"):
            result = copy.deepcopy(R.MAIN if path.endswith(str(R.MAIN_ID)) else R.TAGS)
            if self.fault == "protection":
                result["bypass_actors"] = [{"actor_id": 1}]
        elif path == "immutable-releases":
            result = {"enabled": self.fault != "disabled"}
        elif path == "releases/latest":
            result = {"id": 1, "tag_name": self.tag if self.fault == "latest" else "v0.1.0"}
        elif path == "branches/main":
            result = {"name": "main", "protected": True, "commit": {"sha":
                "0" * 40 if self.fault == "main" else TARGET}}
        elif path == "git/ref/" + self.source_ref.removeprefix("refs/"):
            result = {"ref": self.source_ref, "object": {"type": "commit", "sha": self.approved["commit"]}}
        elif path.startswith("git/commits/"):
            result = {"sha": self.approved["commit"], "tree": {"sha": self.approved["tree"]}}
        elif path.startswith("compare/"):
            result = {"status": "ahead", "behind_by": 0, "ahead_by": 1,
                      "merge_base_commit": {"sha": self.approved["base"]}}
        elif path.startswith("git/ref/tags/"):
            if not self.state["tag"] and self.fault != "collision":
                code = 401 if self.fault == "auth" else 404
                raise subprocess.CalledProcessError(1, arguments, stderr=f"gh: absent (HTTP {code})".encode())
            result = {"ref": "refs/tags/" + self.tag, "object": {"type": "commit", "sha": TARGET}}
        elif path.startswith("releases/assets/"):
            asset_id = int(path.rsplit("/", 1)[1])
            name = (next(name for name, value in P.RECOVERY_ASSETS.items() if value == asset_id)
                    if self.recovery else list(self.uploaded)[asset_id - 1])
            return b"corrupt" if self.fault == "download" else self.uploaded[name]
        elif path.startswith("releases/"):
            if path.startswith("releases/tags/") or not self.state["release"]:
                raise subprocess.CalledProcessError(1, arguments, stderr=b"gh: absent (HTTP 404)")
            result = self.release()
            if self.fault == "assets":
                result["assets"][0]["name"] = "unexpected"
        else:
            raise AssertionError(path)
        return json.dumps(result).encode()


class TransactionTests(unittest.TestCase):
    def simulate(self, role="mise", fault=None, recovery=False):
        approved, stage_assets, _, _, _, _, git = fixture(role)
        github = FakeGithub(self, approved, role, fault, recovery)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            for name, data in stage_assets.items():
                (root / name).write_bytes(data)
            with patch.object(records, "REVISIONS", {(role, approved["commit"]): approved["revision"]}), \
                    patch.object(S, "git", side_effect=git):
                manifest, assets = S.prepare(root, root, role, github.source_ref, approved["commit"], TARGET)
                if recovery:
                    github.uploaded.update(assets)
                with patch.object(P, "gh", side_effect=github), patch.object(P, "RECOVERY_MANIFEST_SHA256",
                        S.sha(assets["source-publication.json"])):
                    operation = P.recover_retained_mise_draft if recovery else P.publish
                    if fault:
                        with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                            operation(manifest, assets)
                    else:
                        result = operation(manifest, assets)
                        self.assertFalse(result["qualified"])
                        self.assertEqual(result["status"], "PUBLISHED_SOURCE_ONLY")
        return github.commands

    def test_both_source_roles_publish_exact_five_immutable_assets(self):
        for role in ("mise", "mbx-action"):
            commands = self.simulate(role)
            uploads = [command for command in commands if command[:2] == ("release", "upload")]
            self.assertEqual(len(uploads), 5)
            self.assertEqual({Path(command[3]).name for command in uploads},
                S.STAGE_NAMES | {"source.commit", "source-publication.json"})
            self.assertFalse(any("--clobber" in command for command in commands))
            create = next(command for command in commands if command[:4] ==
                          ("api", "--method", "POST", P.PREFIX + "releases"))
            self.assertIn("--input", create)
            self.assertEqual(sum(command[:3] == ("api", "--method", "PATCH") for command in commands), 1)
            self.assertGreaterEqual(sum(command == ("api", P.PREFIX + "rulesets/" + str(R.MAIN_ID))
                                        for command in commands), 3)

    def test_forged_manifest_claims_never_reach_github(self):
        approved, stage_assets, _, _, _, _, git = fixture()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            for name, data in stage_assets.items():
                (root / name).write_bytes(data)
            with patch.object(records, "REVISIONS", {("mise", approved["commit"]): approved["revision"]}), \
                    patch.object(S, "git", side_effect=git):
                manifest, assets = S.prepare(root, root, "mise",
                    "refs/heads/owned-source/mise/" + approved["commit"], approved["commit"], TARGET)
                mutations = [lambda m: m.update(qualified=True), lambda m: m.update(schema=True),
                    lambda m: m["raw_commit"].update(cryptographic_signature=True),
                    lambda m: m.update(behavioral_qualification={"passed": True}),
                    lambda m: m["assets"]["source.tar"].update(sha256="0" * 64)]
                for mutation in mutations:
                    candidate, payloads = copy.deepcopy(manifest), dict(assets)
                    mutation(candidate)
                    payloads["source-publication.json"] = (json.dumps(candidate, indent=2, sort_keys=True) + "\n").encode()
                    with patch.object(P, "gh") as command, self.assertRaises(ValueError):
                        P.publish(candidate, payloads)
                    command.assert_not_called()

    def test_created_draft_never_uses_published_tag_discovery(self):
        commands = self.simulate()
        creation = next(i for i, command in enumerate(commands) if command[:4] ==
                        ("api", "--method", "POST", P.PREFIX + "releases"))
        self.assertFalse(any(command[0] == "api" and "releases/tags/" in command[1]
                             for command in commands[creation + 1:]))

    def test_retained_exact_id_recovery_only_promotes(self):
        commands = self.simulate(recovery=True)
        mutations = [command for command in commands if command[:2] == ("api", "--method")]
        self.assertEqual(len(mutations), 1)
        self.assertEqual(mutations[0][:4], ("api", "--method", "PATCH", P.PREFIX + "releases/402229309"))
        self.assertFalse(any(command[0] == "release" for command in commands))
        for fault in ("download", "assets", "protection", "disabled"):
            commands = self.simulate(fault=fault, recovery=True)
            self.assertFalse(any(command[:3] == ("api", "--method", "PATCH") for command in commands))

    def test_preflight_failures_perform_no_writes(self):
        for fault in ("disabled", "protection", "main", "collision", "auth", "latest"):
            commands = self.simulate(fault=fault)
            self.assertFalse(any(command[:2] == ("api", "--method") for command in commands))
            self.assertFalse(any(command[0] == "release" for command in commands))

    def test_race_asset_changes_or_corrupt_download_never_promote(self):
        for fault in ("tag_race", "assets", "download"):
            commands = self.simulate(fault=fault)
            self.assertFalse(any(command[:3] == ("api", "--method", "PATCH") for command in commands))

    def test_final_immutable_status_required(self):
        commands = self.simulate(fault="final_immutable")
        self.assertTrue(any(command[:3] == ("api", "--method", "PATCH") for command in commands))


if __name__ == "__main__":
    unittest.main()

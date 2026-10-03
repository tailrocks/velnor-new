"""Isolated publication policy tests; never contact GitHub or execute tools."""

import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import immutable_publication as I
import owned_tool_source as S

from owned_tool_execution_test_fixtures import P, fixture, archive, binary, receipt_bytes, attestation, environment


class ValidationTests(unittest.TestCase):
    def test_complete_owned_manifest(self):
        for tool in ("mise",):
            manifest, files = fixture(tool)
            P.validate_manifest(manifest)
            for artifact in manifest["artifacts"]:
                P.validate_archive(files[artifact["name"]], artifact, tool, manifest["source"])

    def test_closed_manifest_and_full_targets(self):
        manifest, _ = fixture()
        mutations = [lambda m: m.update(url="https://attacker"),
                     lambda m: m.update(schema=True),
                     lambda m: m["workflow"].update(recipe_sha256="0" * 64),
                     lambda m: m["source"].update(command="evil"),
                     lambda m: m["artifacts"].pop(),
                     lambda m: m["artifacts"].__setitem__(1, m["artifacts"][0]),
                     lambda m: m["artifacts"][0].update(name="../mbx"),
                     lambda m: m["artifacts"][0]["qualification"].update(passed=1)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                candidate = copy.deepcopy(manifest)
                mutation(candidate)
                with self.assertRaises(ValueError):
                    P.validate_manifest(candidate)
        with self.assertRaises(ValueError):
            P.parse_json('{"schema":1,"schema":1}')

    def test_wrong_architecture_license_and_digest(self):
        manifest, files = fixture("mbx")
        artifact = manifest["artifacts"][0]
        bad = archive("mbx", list(P.TARGETS)[1])
        artifact["archive_sha256"] = P.sha(bad)
        artifact["binary_sha256"] = P.sha(binary(list(P.TARGETS)[1]))
        with self.assertRaisesRegex(ValueError, "architecture"):
            P.validate_archive(bad, artifact, "mbx", manifest["source"])
        manifest, files = fixture("mbx")
        artifact = manifest["artifacts"][0]
        manifest["source"]["license_files"]["LICENSE"] = "0" * 64
        with self.assertRaisesRegex(ValueError, "license"):
            P.validate_archive(files[artifact["name"]], artifact, "mbx", manifest["source"])
        with self.assertRaisesRegex(ValueError, "archive digest"):
            P.validate_archive(b"invalid", artifact, "mbx", manifest["source"])

    def test_unsafe_and_duplicate_members(self):
        manifest, _ = fixture("mbx")
        artifact = manifest["artifacts"][0]
        for name in ("../escape", "/absolute", "mbx", "arbitrary", "./mbx"):
            data = archive("mbx", artifact["target"], (name, b"x", 0o755))
            artifact["archive_sha256"] = P.sha(data)
            with self.subTest(name=name), self.assertRaises(ValueError):
                P.validate_archive(data, artifact, "mbx", manifest["source"])

    def test_archive_links_forbidden(self):
        manifest, _ = fixture("mbx")
        artifact = manifest["artifacts"][0]
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
            buffer = io.BytesIO()
            with tarfile.open(fileobj=buffer, mode="w:gz") as output:
                entry = tarfile.TarInfo("mbx")
                entry.type, entry.linkname, entry.mode = kind, "/bin/sh", 0o755
                output.addfile(entry)
            artifact["archive_sha256"] = P.sha(buffer.getvalue())
            with self.assertRaises(ValueError):
                P.validate_archive(buffer.getvalue(), artifact, "mbx", manifest["source"])

    def test_regular_single_link_path_policy(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            file = root / "artifact"
            file.write_bytes(b"safe")
            self.assertEqual(P.read_regular(file), b"safe")
            os.link(file, root / "hard")
            with self.assertRaises(ValueError):
                P.read_regular(file)
            (root / "hard").unlink()
            (root / "link").symlink_to(file)
            with self.assertRaises(OSError):
                P.read_regular(root / "link")
            (root / "directory-link").symlink_to(root, target_is_directory=True)
            with self.assertRaises(OSError):
                P.read_regular(root / "directory-link" / "artifact")

    def test_attestation_bound_to_every_identity(self):
        manifest, _ = fixture()
        artifact = manifest["artifacts"][0]
        proof = attestation(manifest, artifact)
        with patch.object(P, "gh", return_value=json.dumps(proof).encode()) as command:
            P.verify_attestation(Path("artifact"), manifest, artifact)
        args = command.call_args.args
        for flag, value in (("--repo", P.REPO), ("--signer-digest", "a" * 40),
                            ("--source-digest", "a" * 40),
                            ("--source-ref", "refs/heads/main"),
                            ("--predicate-type", P.PREDICATE)):
            self.assertEqual(args[args.index(flag) + 1], value)
        self.assertIn("--deny-self-hosted-runners", args)
        mutations = [lambda s: s["predicate"]["source"].update(commit="a" * 40),
                     lambda s: s["predicate"]["workflow"].update(run_attempt="2"),
                     lambda s: s["predicate"].update(schema=True),
                     lambda s: s["predicate"]["qualification"].update(passed=1),
                     lambda s: s["predicate"]["qualification"].update(sourceartifact_execution_sha256="f" * 64),
                     lambda s: s["subject"][0]["digest"].update(sha256="0" * 64)]
        for mutation in mutations:
            proof = copy.deepcopy(attestation(manifest, artifact))
            mutation(proof[0]["verificationResult"]["statement"])
            with patch.object(P, "gh", return_value=json.dumps(proof).encode()):
                with self.assertRaises(ValueError):
                    P.verify_attestation(Path("artifact"), manifest, artifact)

    def test_source_receipt_closed_staging_contract(self):
        manifest, _ = fixture()
        receipt = json.loads(receipt_bytes(manifest))
        mutations = [lambda r: r.update(schema=True), lambda r: r.update(schema=2),
            lambda r: r.update(upstream_base_commit="0" * 40),
            lambda r: r.update(upstream_repository="https://github.com/attacker/tool"),
            lambda r: r.update(required_hosts=[]), lambda r: r.update(publication={}),
            lambda r: r.update(behavioral_qualification={"passed": True}),
            lambda r: r.update(signed_build_provenance={}), lambda r: r.update(extra=True),
            lambda r: r["source_archive"].update(name="other.tar"),
            lambda r: r["lockfile"].update(path="other.lock"),
            lambda r: r["base_patch"].update(sha256="0" * 64)]
        invalid = []
        for mutation in mutations:
            candidate = copy.deepcopy(receipt)
            mutation(candidate)
            invalid.append(json.dumps(candidate).encode())
        invalid.append(receipt_bytes(manifest).replace(b'"schema": 1', b'"schema": 1, "schema": 1'))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            for data in invalid:
                candidate = copy.deepcopy(manifest)
                candidate["source"]["receipt_sha256"] = P.sha(data)
                (root / "source-receipt.json").write_bytes(data)
                with self.subTest(data=data), patch.object(P, "gh") as command:
                    with self.assertRaises(ValueError):
                        I.stage_evidence(root, candidate, root, P.read_regular, P.gh,
                                         P.verify_attestation)
                    command.assert_not_called()

    def test_release_asset_ids_are_typed_unique_and_complete(self):
        self.assertEqual(I.asset_ids([{"name": "a", "id": 1}], {"a"}), {"a": 1})
        for assets in ([{"name": "a", "id": True}], [{"name": "a", "id": 0}],
                       [{"name": "wrong", "id": 1}],
                       [{"name": "a", "id": 1}, {"name": "b", "id": 1}]):
            with self.assertRaises(ValueError):
                I.asset_ids(assets, {"a", "b"} if len(assets) == 2 else {"a"})

    def test_owned_source_or_tool_latest_rejected(self):
        for tag in ("owned-source-mise-" + "a" * 40, "mise-v1.2.3-owned-test", "mbx-v1.2.3-owned-test"):
            with patch.object(P, "gh", return_value=json.dumps({"id": 1, "tag_name": tag}).encode()):
                with self.assertRaises(ValueError):
                    I.generator_latest(P.gh)

    def test_only_404_allows_absence(self):
        for code in (401, 403, 500, 404):
            error = subprocess.CalledProcessError(1, ["gh"], stderr=f"gh: failed (HTTP {code})".encode())
            with patch.object(P, "gh", side_effect=error):
                if code == 404:
                    I.require_absent(P.gh, "endpoint")
                else:
                    with self.assertRaises(ValueError):
                        I.require_absent(P.gh, "endpoint")
        with patch.object(P, "gh", return_value=b"{}"), self.assertRaises(ValueError):
            I.require_absent(P.gh, "endpoint")


class PublicationTests(unittest.TestCase):
    def simulate(self, corrupt=False, existing=False, tag_race=False, disabled=False,
                 evidence_corrupt=False, immutable=False):
        manifest, files = fixture()
        commands, uploaded = [], {}
        state = {"created": False, "draft": True}
        def release():
            return {"id": 42, "draft": state["draft"], "immutable": not immutable,
                "target_commitish": manifest["workflow_commit"], "prerelease": False,
                "tag_name": manifest["tag"], "assets": [{"id": i + 1, "name": name}
                for i, name in enumerate(uploaded)]}
        def run(argv, **kwargs):
            self.assertFalse(kwargs.get("shell", False))
            self.assertEqual(kwargs["env"]["GH_HOST"], "github.com")
            commands.append(argv)
            args, data = argv[1:], b""
            if args[:2] == ["attestation", "verify"]:
                name = Path(args[2]).name.removeprefix("download-")
                item = next(a for a in manifest["artifacts"] if a["name"] == name)
                data = json.dumps(attestation(manifest, item)).encode()
            elif args[:2] == ["attestation", "download"]:
                name = Path(args[2]).name
                item = next(a for a in manifest["artifacts"] if a["name"] == name)
                (Path(kwargs["cwd"]) / ("sha256:" + item["archive_sha256"] + ".jsonl")).write_bytes(b"signed-bundle")
            elif args[:4] == ["api", "--method", "POST", "repos/" + P.REPO + "/releases"]:
                request = json.loads(Path(args[args.index("--input") + 1]).read_text())
                self.assertTrue(request["draft"])
                self.assertEqual(request["make_latest"], "false")
                self.assertEqual(request["target_commitish"], manifest["workflow_commit"])
                state["created"] = True
                data = json.dumps(release()).encode()
            elif args[:4] == ["api", "--method", "POST", "repos/" + P.REPO + "/git/refs"]:
                data = b"{}"
            elif args[:2] == ["release", "upload"]:
                uploaded[Path(args[3]).name] = Path(args[3]).read_bytes()
            elif args[:3] == ["api", "--method", "PATCH"]:
                state["draft"] = False
            elif args[0] == "api":
                endpoint = args[1]
                if endpoint.endswith("immutable-releases"):
                    data = json.dumps({"enabled": not disabled}).encode()
                elif endpoint.endswith("releases/latest"):
                    data = json.dumps({"id": 1, "tag_name": "v0.1.0"}).encode()
                elif not state["created"]:
                    return subprocess.CompletedProcess(argv, 0 if existing else 1,
                        stdout=b"{}", stderr=b"gh: Not Found (HTTP 404)")
                elif "git/ref/tags/" in endpoint:
                    data = json.dumps({"object": {"type": "commit", "sha":
                        "0" * 40 if tag_race else manifest["workflow_commit"]}}).encode()
                elif "releases/assets/" in endpoint:
                    name = list(uploaded)[int(endpoint.rsplit("/", 1)[1]) - 1]
                    bad = corrupt or (evidence_corrupt and name == "source-receipt.json")
                    data = b"corrupt" if bad else uploaded[name]
                elif "releases/tags/" in endpoint:
                    return subprocess.CompletedProcess(argv, 1, stdout=b"{}", stderr=b"gh: Not Found (HTTP 404)")
                else:
                    data = json.dumps(release()).encode()
            return subprocess.CompletedProcess(argv, 0, stdout=data, stderr=b"")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "manifest.json").write_text(json.dumps(manifest))
            (root / "source-receipt.json").write_bytes(receipt_bytes(manifest))
            for name, data in files.items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_bytes(data)
            env = environment(manifest)
            with patch.dict(os.environ, env), patch.object(P.subprocess, "run", side_effect=run):
                if any((corrupt, existing, tag_race, disabled, evidence_corrupt, immutable)):
                    with self.assertRaises(ValueError):
                        P.publish(root / "manifest.json", root)
                else:
                    P.publish(root / "manifest.json", root)
        return commands

    def test_publish_verifies_download_before_promotion(self):
        commands = self.simulate()
        self.assertEqual(sum(c[1:3] == ["attestation", "verify"] for c in commands), 9)
        promotion = next(c for c in commands if c[1:4] == ["api", "--method", "PATCH"])
        self.assertIn("draft=false", promotion)
        self.assertEqual(promotion[promotion.index("--raw-field") + 1], "make_latest=false")
        self.assertEqual(commands[-1][1], "api")
        uploads = [Path(c[4]).name for c in commands if c[1:3] == ["release", "upload"]]
        self.assertEqual(len(uploads), 23)
        self.assertIn("owned-tool-manifest.json", uploads)
        self.assertIn("source-receipt.json", uploads)
        create = next(c for c in commands if c[1:5] == ["api", "--method", "POST", "repos/" + P.REPO + "/releases"])
        self.assertIn("--input", create)
        creation = commands.index(create)
        self.assertFalse(any("releases/tags/" in str(c) for c in commands[creation + 1:]))
        self.assertFalse(any("--clobber" in c for c in commands))

    def test_failed_verification_never_promotes(self):
        for options in ({"corrupt": True}, {"existing": True}, {"tag_race": True},
                        {"disabled": True}, {"evidence_corrupt": True}):
            commands = self.simulate(**options)
            self.assertFalse(any(c[1:4] == ["api", "--method", "PATCH"] for c in commands))

    def test_published_release_must_report_immutable(self):
        commands = self.simulate(immutable=True)
        self.assertTrue(any(c[1:4] == ["api", "--method", "PATCH"] for c in commands))

    def test_untrusted_dispatch_rejected(self):
        manifest, _ = fixture()
        with patch.dict(os.environ, {}, clear=True), self.assertRaises(ValueError):
            P.trusted_dispatch(manifest)


if __name__ == "__main__":
    unittest.main()

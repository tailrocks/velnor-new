"""Same authenticated ZIP across anonymous proof, publishers, and reconciliation."""
import copy
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile

ROOT = Path(__file__).resolve().parents[1] / "src"
COMMON = ROOT.parents[1] / "velnor-actions-orchestrator" / "src"
FIXTURE = {"__file__": str(Path(__file__).with_name("release_publish_test_fixture.py"))}
exec(compile(Path(FIXTURE["__file__"]).read_text(), "fixture", "exec"), FIXTURE)
RUST_MODULES = ("release_reconcile_cargo.py", "release_package_contract.py", "release_reconcile_registry.py",
                "release_publish_metadata.py", "release_publish_manifest.py", "release_publish_verify.py",
                "release_publish_artifact.py", "release_publish_transport.py", "release_publish_auth.py",
                "release_publish_registry.py", "release_publish_entry.py")


def namespace(extra=()):
    value = {"__name__": "crossphase"}
    sources = [COMMON / "release_reconcile_common.py", *(ROOT / name for name in RUST_MODULES),
               COMMON / "release_reconcile_forge.py", COMMON / "release_publish_proof.py",
               *(COMMON / name for name in extra)]
    for source in sources:
        exec(compile(source.read_text(), str(source), "exec"), value)
    return value


NS = namespace()
FORGE = namespace(("release_forge_publish_read.py", "release_forge_publish_verify.py",
                   "release_forge_publish_api.py", "release_forge_publish.py"))
RECONCILE = namespace(("release_forge_publish_read.py", "release_forge_publish_verify.py",
                       "release_reconcile_entry.py"))


def approved(names):
    return {"schema": 1, "repository": "owner/repo", "registry": "crates-io",
            "source_sha": FIXTURE["SHA"], "packages": {name: "1.0.0" for name in names},
            "owners": {name: ["user:1"] for name in names},
            "tags": {name: f"{name}-v1.0.0" for name in names},
            "authentication": "bootstrap-token", "intent_id": "crossphase",
            "tools": {key: "1.0.0" for key in ("generator", "release-plz", "rust", "python", "gh")}}


def receipt_zip(receipt):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w") as archive:
        archive.writestr("receipt.json", json.dumps(receipt))
    return stream.getvalue()


class Remote:
    def __init__(self, policy, blob, archives):
        self.policy, self.archives = policy, archives
        self.artifacts, self.tags, self.refs, self.releases = {}, {}, {}, {}
        self.calls, self.writes = [], []
        self.add_artifact("package", blob, 11, "release-package")
        self.failure = None

    def add_artifact(self, kind, blob, identifier, producer, conclusion="success"):
        name = f"velnor-release-{kind}-r123-a2"
        digest = hashlib.sha256(blob).hexdigest()
        self.artifacts[identifier] = (blob, {"id": identifier, "name": name,
            "digest": "sha256:" + digest, "size_in_bytes": len(blob), "expired": False,
            "workflow_run": {"id": 123, "head_sha": FIXTURE["SHA"],
                             "repository_id": 1, "head_repository_id": 1}}, producer, conclusion)
        prefix = {"package": "RELEASE_PACKAGE_ARTIFACT", "registry": "RELEASE_REGISTRY_RECEIPT_ARTIFACT",
                  "forge": "RELEASE_FORGE_RECEIPT_ARTIFACT"}[kind]
        return {prefix + "_ID": str(identifier), prefix + "_DIGEST": digest}

    def run(self, command, **kwargs):
        assert command[:5] == ["gh", "api", "--hostname", "github.com", "--include"] or \
            command[:4] == ["gh", "api", "--hostname", "github.com"]
        endpoint = command[-1]
        self.calls.append(endpoint)
        if endpoint.endswith("/zip"):
            identifier = int(endpoint.split("/")[-2])
            return subprocess.CompletedProcess(command, 0, self.artifacts[identifier][0], b"")
        value = self.observe(endpoint)
        status = 404 if value is None else 200
        content = f"HTTP/2 {status}\r\n\r\n".encode() + json.dumps(value).encode()
        return subprocess.CompletedProcess(command, int(status == 404), content, b"")

    def observe(self, endpoint):
        if endpoint.endswith("/attempts/2"):
            repository = {"id": 1, "full_name": "owner/repo"}
            return {"id": 123, "run_attempt": 2, "head_sha": FIXTURE["SHA"],
                    "repository": repository, "head_repository": repository, "workflow_id": 7}
        if endpoint.endswith("/workflows/7"):
            return {"path": ".github/workflows/release.yml"}
        if "/jobs?" in endpoint:
            return {"jobs": [{"id": identifier + 100, "name": producer, "run_id": 123,
                "run_attempt": 2, "head_sha": FIXTURE["SHA"], "status": "completed",
                "conclusion": conclusion} for identifier, (_, _, producer, conclusion)
                in self.artifacts.items()]}
        if "/artifacts?" in endpoint:
            return {"artifacts": [item[1] for item in self.artifacts.values()]}
        if "/git/ref/tags/" in endpoint:
            return self.refs.get(endpoint.split("/tags/")[-1])
        if "/git/tags/" in endpoint:
            return self.tags.get(endpoint.rsplit("/", 1)[-1])
        if "/releases/tags/" in endpoint:
            return self.releases.get(endpoint.split("/tags/")[-1])
        raise AssertionError("unexpected forge endpoint: " + endpoint)

    def fetch(self, url, *_args):
        if url.startswith("https://index.crates.io/"):
            name = url.rsplit("/", 1)[-1]
            return json.dumps({"name": name, "vers": "1.0.0", "yanked": False,
                "cksum": hashlib.sha256(self.archives[name]).hexdigest(),
                "features": {}, "deps": []}).encode()
        if url.startswith("https://static.crates.io/"):
            return self.archives[url.split("/")[-2]]
        name = url.split("/crates/")[-1].split("/")[0]
        if name == self.failure:
            raise NS["ReconcileError"]("local_registry_failure")
        if url.endswith("owner_user"):
            return b'{"users":[{"id":1}]}'
        if url.endswith("owner_team"):
            return b'{"teams":[]}'
        if url.endswith("dependencies"):
            return b'{"dependencies":[]}'
        return json.dumps({"version": {"crate": name, "num": "1.0.0", "yanked": False,
            "checksum": hashlib.sha256(self.archives[name]).hexdigest(), "features": {}}}).encode()

    def create_tag(self, _repository, payload):
        self.writes.append(("tag", copy.deepcopy(payload)))
        sha = hashlib.sha1(payload["tag"].encode()).hexdigest()
        value = {"sha": sha, "tag": payload["tag"], "message": payload["message"],
                 "object": {"sha": payload["object"], "type": "commit"}}
        self.tags[sha] = value
        return value

    def create_ref(self, _repository, payload):
        self.writes.append(("ref", copy.deepcopy(payload)))
        value = {"ref": payload["ref"], "object": {"sha": payload["sha"], "type": "tag"}}
        self.refs[payload["ref"].removeprefix("refs/tags/")] = value
        return value

    def create_release(self, _repository, payload):
        self.writes.append(("release", copy.deepcopy(payload)))
        value = {**payload, "html_url": "https://github.com/owner/repo/releases/tag/" + payload["tag_name"]}
        self.releases[payload["tag_name"]] = value
        return value


class CrossphaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(dir=Path(tempfile.gettempdir()).resolve())
        self.addCleanup(self.temporary.cleanup)
        self.previous = Path.cwd()
        os.chdir(self.temporary.name)
        self.addCleanup(os.chdir, self.previous)
        self.policy = approved(["demo"])
        self.proofs, self.archives = {}, {}
        self.prepare(["demo"])
        self.environment = {**FIXTURE["ENVIRONMENT"], "RUNNER_TEMP": self.temporary.name,
                            "RELEASE_RECONCILE_POLICY": json.dumps(self.policy)}
        self.environment.update(self.remote.add_artifact("package", self.blob, 11, "release-package"))
        self.enterContext(patch.dict(os.environ, self.environment, clear=True))
        self.enterContext(patch.object(subprocess, "run", side_effect=lambda *args, **kw: self.remote.run(*args, **kw)))
        self.forbidden = Mock(side_effect=AssertionError("credentials or network requested"))
        self.enterContext(patch.object(NS["urllib"].request, "build_opener", self.forbidden))
        for value in (NS, FORGE, RECONCILE):
            self.enterContext(patch.dict(value, {"ReconcileError": NS["ReconcileError"], "require": NS["require"], "fetch": lambda *args: self.remote.fetch(*args),
                                                    "registry_token": self.forbidden}))
        for value in (FORGE, RECONCILE):
            self.enterContext(patch.dict(value, {
            "read_tag_ref": lambda repo, tag: self.remote.refs.get(tag),
            "read_tag_object": lambda repo, sha: self.remote.tags.get(sha),
            "read_release": lambda repo, tag: self.remote.releases.get(tag),
            "create_tag": lambda *args: self.remote.create_tag(*args),
            "create_tag_ref": lambda *args: self.remote.create_ref(*args),
            "create_release": lambda *args: self.remote.create_release(*args)}))

    def prepare(self, names):
        self.policy = approved(names)
        pairs = {name: FIXTURE["package"](name) for name in names}
        self.proofs = {name: pair[0] for name, pair in pairs.items()}
        self.archives = {name: pair[1] for name, pair in pairs.items()}
        self.blob = FIXTURE["artifact"](self.proofs, self.archives, self.policy)
        # Same tar content, deliberately different gzip envelope: normalized recovery.
        normalized = {name: gzip.compress(gzip.decompress(data), mtime=0)
                      for name, data in self.archives.items()}
        self.remote = Remote(self.policy, self.blob, normalized)

    def materialize(self):
        NS["create_registry_artifact_proof"]()
        blob = NS["_read_verified_artifact"]()
        self.assertEqual(blob, self.blob)
        return NS["validate_package_artifact"](blob, self.policy)

    def test_same_zip_through_registry_forge_and_reconcile(self):
        candidate, archives = self.materialize()
        self.assertEqual(archives, self.archives)
        root = Path(self.temporary.name) / "velnor/verified-release"
        self.assertEqual(stat.S_IMODE((root / "artifact.zip").stat().st_mode), 0o400)
        identity = json.loads((root / "artifact.json").read_text())
        self.assertEqual(identity["producer_job"], {"id": 111, "name": "release-package", "conclusion": "success"})
        receipt = NS["publish_registry"](self.policy, candidate["packages"], archives)
        NS["_registry_matches_candidate"](receipt, candidate, self.policy)
        operation = receipt["operations"]["demo"]
        self.assertEqual(operation["relation"], "existing-normalized")
        self.assertNotEqual(operation["registry"]["registry_checksum"], self.proofs["demo"]["archive_sha256"])
        os.environ.update(self.remote.add_artifact("registry", receipt_zip(receipt), 12, "release-registry-publish"))
        forge = FORGE["publish_forge"](self.policy)
        self.assertEqual(forge["status"], "verified")
        self.assertEqual([kind for kind, _ in self.remote.writes], ["tag", "ref", "release"])
        self.assertEqual(self.remote.writes[-1][1], candidate["packages"]["demo"]["forge_release"])
        os.environ.update(self.remote.add_artifact("forge", receipt_zip(forge), 13, "release-forge-publish"))
        RECONCILE["reconcile"]()
        final = json.loads(Path("release-receipt/receipt.json").read_text())
        self.assertEqual(final["status"], "verified")
        self.assertEqual(final["registry_receipt"], receipt)
        self.assertEqual(final["forge_receipt"], forge)
        self.assertEqual(final["package_artifact"], identity)
        self.forbidden.assert_not_called()

    def test_tamper_and_stale_binding_fail_before_credentials(self):
        self.materialize()
        root = Path(self.temporary.name) / "velnor/verified-release"
        original = (root / "artifact.zip").read_bytes()
        (root / "artifact.zip").chmod(0o600)
        (root / "artifact.zip").write_bytes(original + b"tamper")
        with self.assertRaisesRegex(NS["ReconcileError"], "registry_verified_digest"):
            NS["_read_verified_artifact"]()
        with self.assertRaises(SystemExit):
            NS["bootstrap_publish_main"]()
        (root / "artifact.zip").write_bytes(original)
        os.environ["RELEASE_PACKAGE_ARTIFACT_ID"] = "99"
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_proof_identity"):
            NS["_read_verified_artifact"]()
        self.forbidden.assert_not_called()

    def test_identity_and_namespace_are_closed(self):
        self.materialize()
        root = Path(self.temporary.name) / "velnor/verified-release"
        identity = json.loads((root / "artifact.json").read_text())
        identity["producer_job"] = {"job_id": 111, "name": "release-package", "status": "success"}
        (root / "artifact.json").write_text(json.dumps(identity))
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_proof_producer"):
            NS["_read_verified_artifact"]()
        renamed = root.with_name("renamed")
        root.rename(renamed)
        root.symlink_to(renamed, target_is_directory=True)
        with self.assertRaisesRegex(NS["ReconcileError"], "registry_verified_namespace"):
            NS["_read_verified_artifact"]()
        self.forbidden.assert_not_called()

    def test_submitted_relation_requires_exact_compressed_checksum(self):
        candidate, archives = self.materialize()
        receipt = NS["publish_registry"](self.policy, candidate["packages"], archives)
        receipt["operations"]["demo"]["relation"] = "submitted-exact"
        with self.assertRaisesRegex(NS["ReconcileError"], "registry_operation_submitted_checksum"):
            NS["_registry_matches_candidate"](receipt, candidate, self.policy)
        self.forbidden.assert_not_called()

    def test_partial_publisher_keeps_verified_first_package_and_durable_failure(self):
        self.prepare(["alpha", "beta"])
        os.environ["RELEASE_RECONCILE_POLICY"] = json.dumps(self.policy)
        os.environ.update(self.remote.add_artifact("package", self.blob, 11, "release-package"))
        candidate, archives = self.materialize()
        self.remote.failure = "beta"
        receipt = NS["publish_registry"](self.policy, candidate["packages"], archives)
        persisted = json.loads(Path("release-registry/receipt.json").read_text())
        self.assertEqual(persisted, receipt)
        self.assertEqual(receipt["status"], "incomplete")
        self.assertEqual(receipt["operations"]["alpha"]["status"], "verified")
        self.assertEqual(receipt["operations"]["beta"]["status"], "failed")
        os.environ.update(self.remote.add_artifact("registry", receipt_zip(receipt), 12,
                                                 "release-registry-publish", "failure"))
        loaded, identity = NS["load_publish_receipt"](self.policy, "registry")
        self.assertEqual(loaded, receipt)
        self.assertEqual(identity["producer_job"]["conclusion"], "failure")
        with self.assertRaisesRegex(NS["ReconcileError"], "registry_producer_failed"):
            FORGE["load_forge_publish_input"](self.policy)
        self.assertEqual(self.remote.writes, [])
        self.forbidden.assert_not_called()

    def test_partial_forge_and_reconcile_preserve_success_and_terminal_inputs(self):
        self.prepare(["alpha", "beta"])
        os.environ["RELEASE_RECONCILE_POLICY"] = json.dumps(self.policy)
        os.environ.update(self.remote.add_artifact("package", self.blob, 11, "release-package"))
        candidate, archives = self.materialize()
        registry = NS["publish_registry"](self.policy, candidate["packages"], archives)
        os.environ.update(self.remote.add_artifact("registry", receipt_zip(registry), 12,
                                                 "release-registry-publish"))
        create = self.remote.create_release

        def fail_second(repository, payload):
            if payload["tag_name"] == "beta-v1.0.0":
                raise NS["ReconcileError"]("local_forge_failure")
            return create(repository, payload)

        with patch.object(self.remote, "create_release", side_effect=fail_second):
            forge = FORGE["publish_forge"](self.policy)
        self.assertEqual(forge["status"], "incomplete")
        self.assertEqual(forge["operations"]["alpha"]["status"], "verified")
        self.assertEqual(forge["operations"]["beta"]["status"], "failed")
        self.assertEqual(forge["operations"]["beta"]["tag"]["source_sha"], self.policy["source_sha"])
        self.assertNotIn("release", forge["operations"]["beta"])
        self.assertEqual(json.loads(Path("release-forge/receipt.json").read_text()), forge)
        os.environ.update(self.remote.add_artifact("forge", receipt_zip(forge), 13,
                                                 "release-forge-publish", "failure"))
        with self.assertRaisesRegex(NS["ReconcileError"], "partial_or_failed_reconciliation"):
            RECONCILE["reconcile"]()
        final = json.loads(Path("release-receipt/receipt.json").read_text())
        self.assertEqual(final["status"], "incomplete")
        self.assertEqual(final["operations"]["alpha"]["status"], "verified")
        self.assertEqual(final["operations"]["beta"]["status"], "failed")
        self.assertEqual(final["operations"]["beta"]["registry"]["status"], "verified")
        self.assertEqual(final["registry_receipt"], registry)
        self.assertEqual(final["forge_receipt"], forge)
        self.assertEqual(final["forge_artifact"]["producer_job"]["conclusion"], "failure")
        self.forbidden.assert_not_called()


if __name__ == "__main__":
    unittest.main()

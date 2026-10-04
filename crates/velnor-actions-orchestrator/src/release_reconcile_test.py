"""Adversarial publication proof tests, without network, Cargo, or Git."""

import hashlib
import io
import json
import os
import re
import subprocess
from pathlib import Path
import tempfile
import unittest
import zipfile
from types import SimpleNamespace
from unittest.mock import patch


ROOT = Path(__file__).parent
RUST = ROOT.parent.parent / "velnor-actions-rust" / "src"
NS = {"__name__": "reconcile_test"}
SOURCES = (
    (ROOT, "release_reconcile_common.py"),
    (RUST, "release_reconcile_cargo.py"),
    (RUST, "release_package_contract.py"),
    (RUST, "release_reconcile_registry.py"),
    (RUST, "release_publish_metadata.py"),
    (RUST, "release_publish_manifest.py"),
    (RUST, "release_publish_verify.py"),
    (RUST, "release_publish_artifact.py"),
    (ROOT, "release_reconcile_forge.py"),
    (ROOT, "release_publish_proof.py"),
    (ROOT, "release_forge_publish_read.py"),
    (ROOT, "release_forge_publish_verify.py"),
    (ROOT, "release_reconcile_entry.py"),
)
for owner, filename in SOURCES:
    path = owner / filename
    exec(compile(path.read_text(), str(path), "exec"), NS)

SHA = "a" * 40
POLICY = {
    "schema": 1,
    "repository": "owner/repo",
    "registry": "crates-io",
    "source_sha": SHA,
    "packages": {"demo": "1.0.0"},
    "owners": {"demo": ["team:2", "user:1"]},
    "tags": {"demo": "demo-v1.0.0"},
    "authentication": "trusted-publishing",
    "tools": {key: "1.0.0" for key in ("generator", "release-plz", "rust", "python", "gh")},
    "intent_id": "intent-1",
}

class ForgeCollisionTests(unittest.TestCase):
    def test_orphan_release_and_tag_collision(self):
        with patch.dict(NS, {"forge_api": lambda endpoint: None if "/git/ref/" in endpoint else {}}):
            with self.assertRaisesRegex(NS["ReconcileError"], "release_without_tag_collision"):
                NS["forge_package"](POLICY, "demo", False)
        ref = {"ref": "refs/tags/demo-v1.0.0", "object": {"type": "commit", "sha": "b" * 40}}
        with patch.dict(NS, {"forge_api": lambda _: ref}):
            with self.assertRaisesRegex(NS["ReconcileError"], "tag_source_collision"):
                NS["forge_package"](POLICY, "demo", False)

class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.content = b'{"schema":1}'
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr("evidence.json", self.content)
        self.blob = stream.getvalue()
        self.artifact = {
            "id": 42,
            "name": "velnor-release-preflight-r123-a2",
            "expired": False,
            "size_in_bytes": len(self.blob),
            "digest": "sha256:" + hashlib.sha256(self.blob).hexdigest(),
            "workflow_run": {
                "id": 123, "run_attempt": 2, "head_sha": SHA,
                "repository_id": 1, "head_repository_id": 1,
            },
        }

    def api(self, endpoint):
        if endpoint.endswith("attempts/2"):
            return {
                "id": 123, "run_attempt": 2, "head_sha": SHA,
                "repository": {"full_name": "owner/repo", "id": 1},
                "head_repository": {"full_name": "owner/repo", "id": 1},
                "workflow_id": 9,
            }
        if endpoint.endswith("workflows/9"):
            return {"path": ".github/workflows/release.yml"}
        if endpoint.endswith("/jobs?per_page=100&page=1"):
            return {"jobs": [{
                "id": 77, "name": "release-preflight", "run_id": 123,
                "run_attempt": 2, "head_sha": SHA, "status": "completed",
                "conclusion": "success",
            }]}
        return {"artifacts": [self.artifact]}

    def proof(self):
        environment = {
            "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2", "GITHUB_SHA": SHA,
            "RELEASE_PREFLIGHT_ARTIFACT_ID": "42",
            "RELEASE_PREFLIGHT_ARTIFACT_DIGEST": self.artifact["digest"].removeprefix("sha256:"),
        }
        with patch.dict(os.environ, environment), \
             patch.dict(NS, {"forge_api": self.api}), \
             patch.object(NS["subprocess"], "run", return_value=SimpleNamespace(
                 returncode=0, stdout=self.blob
             )):
            return NS["artifact_evidence"](POLICY)

    def test_authenticated_single_json_archive(self):
        content, receipt = self.proof()
        self.assertEqual(content, self.content)
        self.assertEqual(receipt["id"], 42)
        self.assertEqual(receipt["producer_job"], {
            "id": 77, "name": "release-preflight", "conclusion": "success",
        })

    def test_digest_origin_expiry_and_member_set_fail(self):
        self.artifact["digest"] = "sha256:" + "0" * 64
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_digest_mismatch"):
            self.proof()
        self.artifact["digest"] = "sha256:" + hashlib.sha256(self.blob).hexdigest()
        self.artifact["workflow_run"]["head_sha"] = "b" * 40
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_origin"):
            self.proof()
        self.artifact["workflow_run"]["head_sha"] = SHA
        self.artifact["expired"] = True
        with self.assertRaises(NS["ReconcileError"]):
            self.proof()
        self.artifact["expired"] = False
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, "w") as archive:
            archive.writestr("evidence.json", self.content)
            archive.writestr("../poison.py", b"raise Exception()")
        self.blob = stream.getvalue()
        self.artifact["size_in_bytes"] = len(self.blob)
        self.artifact["digest"] = "sha256:" + hashlib.sha256(self.blob).hexdigest()
        with self.assertRaisesRegex(NS["ReconcileError"], "artifact_members"):
            self.proof()


class LauncherTests(unittest.TestCase):
    RUST_MODULES = {
        "release_source_validation.py", "release_reconcile_cargo.py",
        "release_package_contract.py", "release_reconcile_registry.py",
        "release_publish_metadata.py", "release_publish_manifest.py",
        "release_publish_verify.py", "release_publish_artifact.py",
        "release_preflight_cargo.py", "release_package.py",
    }

    def module_constant(self, factory, name):
        match = re.search(rf"const {name}:.*?= &\[(.*?)\];", factory, re.DOTALL)
        self.assertIsNotNone(match, name)
        return re.findall(r'"([^"\n]+\.py)"', match.group(1))

    def sealed_body(self, kind):
        factory = (ROOT / "release_support_sources.rs").read_text()
        pure = self.module_constant(factory, "PURE_MODULES")
        fixed = {
            True: self.module_constant(factory, "PACKAGE_MODULES"),
            "forge": pure + [
                "release_reconcile_forge.py", "release_publish_proof.py",
                "release_forge_preflight.py",
            ],
            False: pure + [
                "release_reconcile_forge.py", "release_publish_proof.py",
                "release_forge_publish_read.py", "release_forge_publish_verify.py",
                "release_reconcile_entry.py",
            ],
        }
        names = fixed[kind]
        template = re.search(r'r#"(.*?)"#', factory, re.DOTALL).group(1)
        entry = {
            True: "package_main", "forge": "forge_preflight_main", False: "reconcile_main",
        }[kind]
        template = template.replace("{names}", repr(names)).replace("{entry}", entry)
        template = template.replace("{{", "{").replace("}}", "}")
        sources = {
            name: ((RUST if name in self.RUST_MODULES else ROOT) / name).read_text()
            for name in names
        }
        wrapper = re.search(r'("set -euo pipefail.*?"),', factory).group(1)
        wrapper = json.loads(wrapper)
        encoded = json.dumps(sources, ensure_ascii=False, separators=(",", ":"))
        return wrapper.replace("{}", template).replace("{sources}", encoded)

    def run_sealed(self, workspace, kind=False):
        environment = {
            **os.environ,
            "RELEASE_RECONCILE_POLICY": json.dumps(POLICY),
            "GITHUB_SHA": SHA, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "1",
            "GITHUB_WORKSPACE": str(workspace), "RELEASE_REGISTRY": "crates-io",
            "RELEASE_REPOSITORY": "owner/repo", "RELEASE_MANIFEST": "Cargo.toml",
            "RELEASE_EXPECTED_PACKAGES": json.dumps(POLICY["packages"]),
            "RELEASE_RUST_TOOLCHAIN": "1.98.1", "RELEASE_PUBLISHABLE_WORKSPACE": "0",
        }
        return subprocess.run(
            ["bash", "-s", "--", "--manifest-path", "release-source/Cargo.toml"],
            input=self.sealed_body(kind).encode(), cwd=workspace, env=environment,
            capture_output=True, check=False,
        )

    def test_sealed_closure_ignores_mutated_missing_and_cached_companions(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory).resolve()
            root = workspace / ".github" / "velnor"
            root.mkdir(parents=True)
            for name in self.RUST_MODULES | {
                "json.py", "release_reconcile_common.py", "release_reconcile_entry.py",
                "release_reconcile_forge.py", "release_publish_proof.py",
                "release_forge_publish_read.py", "release_forge_publish_verify.py",
            }:
                (root / name).write_text("raise SystemExit('hostile_companion')")
                (root / (name + "c")).write_bytes(b"hostile bytecode")
            result = self.run_sealed(workspace)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"release_reconcile:ReconcileError:artifact_upload_binding", result.stderr)
            self.assertNotIn(b"hostile_companion", result.stderr)
            receipt = json.loads((workspace / "release-receipt/receipt.json").read_text())
            self.assertEqual(receipt["status"], "incomplete")
            for name in root.iterdir():
                name.unlink()
            result = self.run_sealed(workspace)
            self.assertIn(b"release_reconcile:ReconcileError:artifact_upload_binding", result.stderr)
            self.assertNotIn("sys.path", self.sealed_body(False))
            self.assertNotIn("read_bytes", self.sealed_body(False).split("namespace =", 1)[1])

    def test_package_sealed_closure_routes_fixed_entry(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_sealed(Path(directory).resolve(), True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"release_package:ValidationError:missing_source_root", result.stderr)
            self.assertNotIn(b"release_source_validation:", result.stderr)

    def test_forge_sealed_closure_requires_exact_upload_receipt_before_api(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_sealed(Path(directory).resolve(), "forge")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b"release_forge_preflight:ReconcileError:artifact_upload_binding", result.stderr)


def package(policy, name, version):
    return {
        "files": {}, "features": {}, "archive_sha256": "0" * 64,
        "publish_metadata": {}, "dependencies": [], "cargo_dependency_proofs": [],
        "forge_release": {
            "tag_name": policy["tags"][name], "body": "Release notes.",
            "name": f"{name}-{version}", "draft": False, "prerelease": False,
        },
    }


def candidate(policy=POLICY, packages=None, order=None):
    packages = packages or {
        name: package(policy, name, version)
        for name, version in policy["packages"].items()
    }
    return {
        "schema": 1, "policy": policy, "packages": packages,
        "publication_order": order or list(policy["packages"]),
        "workflow_sha": SHA, "run_id": "123", "run_attempt": "2",
        "status": "package-verified",
    }


def terminal(policy=POLICY, order=None):
    return {
        "schema": 1, "policy": policy, "status": "incomplete",
        "workflow_sha": SHA, "run_id": "123", "run_attempt": "2",
        "publication_order": order or list(policy["packages"]),
        "operations": {
            name: {"version": version, "status": "pending"}
            for name, version in policy["packages"].items()
        },
    }


def artifact(name, identifier, producer, fill):
    return {
        "id": identifier, "digest": "sha256:" + fill * 64,
        "name": name,
        "producer_job": {"id": identifier + 100, "name": producer,
                         "conclusion": "success"},
    }


class ReceiptTests(unittest.TestCase):
    def bindings(self, candidate_value, registry_receipt, forge_receipt):
        package_artifact = artifact(
            "velnor-release-package-r123-a2", 11, "release-package", "1"
        )
        terminal_artifacts = {
            "registry": artifact(
                "velnor-release-registry-r123-a2", 12,
                "release-registry-publish", "2"
            ),
            "forge": artifact(
                "velnor-release-forge-r123-a2", 13,
                "release-forge-publish", "3"
            ),
        }

        def binding(producer):
            values = {
                "release-package": (11, "sha256:" + "1" * 64),
                "release-registry-publish": (12, "sha256:" + "2" * 64),
                "release-forge-publish": (13, "sha256:" + "3" * 64),
            }
            return values[producer]

        return {
            "load_package_input": lambda _approved: (
                candidate_value, {}, package_artifact, b"raw-package-zip"
            ),
            "load_publish_receipt": lambda _approved, kind: (
                registry_receipt if kind == "registry" else forge_receipt,
                terminal_artifacts[kind],
            ),
            "_artifact_upload_binding": binding,
            "validate_artifact_identity": lambda value, *_args: value,
            "verify_published_package": lambda *_args: {"status": "verified"},
            "verify_forge_package": lambda *_args: {"status": "verified"},
        }

    def execute(self, environment, values):
        error = None
        with tempfile.TemporaryDirectory() as directory, \
                patch.dict(os.environ, environment, clear=True), \
                patch.dict(NS, values):
            previous = Path.cwd()
            os.chdir(directory)
            try:
                NS["reconcile"]()
            except NS["ReconcileError"] as raised:
                error = raised
            finally:
                os.chdir(previous)
            receipt = json.loads((Path(directory) / "release-receipt/receipt.json").read_text())
        return error, receipt

    def test_failed_package_retains_incomplete_receipt(self):
        value = candidate()
        values = self.bindings(value, terminal(), terminal())

        def fail(*_args):
            raise NS["ReconcileError"]("wrong_source")

        values["verify_published_package"] = fail
        error, receipt = self.execute({
            "RELEASE_RECONCILE_POLICY": json.dumps(POLICY),
            "GITHUB_SHA": SHA, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2",
        }, values)
        self.assertIsInstance(error, NS["ReconcileError"])
        self.assertEqual(str(error), "partial_or_failed_reconciliation")
        self.assertEqual(receipt["status"], "incomplete")
        self.assertEqual(receipt["operations"]["demo"]["reason"], "wrong_source")

        stale = {**value, "run_attempt": "1"}
        stale_values = self.bindings(stale, terminal(), terminal())
        error, _receipt = self.execute({
            "RELEASE_RECONCILE_POLICY": json.dumps(POLICY),
            "GITHUB_SHA": SHA, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2",
        }, stale_values)
        self.assertIsInstance(error, NS["ReconcileError"])
        self.assertEqual(str(error), "reconcile_package_candidate")

    def test_corrupt_later_package_retains_earlier_success(self):
        approved = {
            **POLICY,
            "packages": {"demo": "1.0.0", "second": "2.0.0"},
            "owners": {"demo": ["user:1"], "second": ["user:1"]},
            "tags": {"demo": "demo-v1.0.0", "second": "second-v2.0.0"},
        }
        value = candidate(approved, order=["demo", "second"])
        values = self.bindings(
            value, terminal(approved, ["demo", "second"]),
            terminal(approved, ["demo", "second"]),
        )

        def registry(_approved, name, *_args):
            if name == "second":
                raise NS["ReconcileError"]("invalid_archive")
            return {"status": "verified"}

        values["verify_published_package"] = registry
        error, receipt = self.execute({
            "RELEASE_RECONCILE_POLICY": json.dumps(approved),
            "GITHUB_SHA": SHA, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2",
        }, values)
        self.assertIsInstance(error, NS["ReconcileError"])
        self.assertEqual(str(error), "partial_or_failed_reconciliation")
        self.assertEqual(receipt["operations"]["demo"]["status"], "verified")
        self.assertEqual(receipt["operations"]["second"]["status"], "failed")
        self.assertEqual(receipt["status"], "incomplete")

if __name__ == "__main__":
    unittest.main()

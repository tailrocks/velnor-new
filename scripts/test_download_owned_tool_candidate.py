"""Mocked GitHub reads and hostile artifact-container admission tests."""

import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch
import zipfile

sys.path.insert(0, str(Path(__file__).parent))
import owned_tool_source as source
import source_qualification_execution as execution


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parent / path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


download = load("owned_download", "download-owned-tool-candidate.py")
builder = load("owned_build", "build-owned-tool.py")
WORKFLOW = {"commit": "f" * 40, "run_id": "123", "run_attempt": "2"}
TARGET = "aarch64-apple-darwin"
NAME = "owned-candidate-123-2-mise-" + TARGET
BINARY = b"\xcf\xfa\xed\xfe\x0c\x00\x00\x01\x00\x00\x00\x00\x02" + b"\0" * 51


def zip_data(contents):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in contents.items():
            archive.writestr(name, data)
    return output.getvalue()


def fixture():
    base = "https://github.com/tailrocks/velnor-new/releases/download/source-stage"
    approved = {"tool": "mise", "version": "2026.10.6-owned-cargo-wrapper",
                "source_commit": "1" * 40, "source_tree": "2" * 40,
                "upstream_base_commit": source.BASES["mise"][1],
                "archive_url": base + "/source.tar", "archive_sha256": "a" * 64,
                "receipt_url": base + "/source-receipt.json", "receipt_sha256": "b" * 64,
                "patch_url": base + "/base.patch", "patch_sha256": "c" * 64,
                "lockfile_sha256": source.digest(b"lock"),
                "license_files": {"LICENSE": source.digest(b"license")}}
    staged = {"schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": "mise",
              "upstream_repository": "https://github.com/jdx/mise",
              "upstream_base_commit": approved["upstream_base_commit"],
              "source_commit": approved["source_commit"], "source_tree": approved["source_tree"],
              "source_archive": {"name": "source.tar", "sha256": approved["archive_sha256"]},
              "base_patch": {"name": "base.patch", "sha256": approved["patch_sha256"]},
              "lockfile": {"path": "Cargo.lock", "sha256": approved["lockfile_sha256"]},
              "license_files": approved["license_files"], "required_hosts": source.HOSTS,
              "publication": None, "behavioral_qualification": None, "signed_build_provenance": None}
    staged_bytes = json.dumps(staged).encode()
    approved["receipt_sha256"] = source.digest(staged_bytes)
    record = {"commit": approved["source_commit"], "tree": approved["source_tree"],
              "archive_sha256": approved["archive_sha256"], "receipt_sha256": approved["receipt_sha256"],
              "lockfile_sha256": approved["lockfile_sha256"], "base_patch_sha256": approved["patch_sha256"],
              "license_files": approved["license_files"]}
    binary_archive = builder.package(BINARY, b"license", "mise")
    archive_name = "mise-" + approved["version"] + "-" + TARGET + ".tar.gz"
    receipt = {"schema": 1, "status": "SOURCE_BUILD_CANDIDATE", "tool": "mise",
               "version": approved["version"], "target": TARGET, "source": record,
               "workflow": {**WORKFLOW, "recipe_sha256": source.recipe_sha("mise")},
               "artifact": {"name": archive_name, "archive_sha256": source.digest(binary_archive),
                            "binary_sha256": source.digest(BINARY)},
               "version_banner": approved["version"], "compiler": {"rustc_vv": "rustc", "linker": "clang"},
               "runner": {"image_os": "macos15", "image_version": "20261003"},
               "recipe": source.recipe("mise"), "behavioral_qualification": None}
    contents = {"candidate-receipt.json": json.dumps(receipt).encode(),
                "source-receipt.json": staged_bytes, archive_name: binary_archive}
    return approved, contents, receipt


def metadata(data):
    return {"id": 987, "name": NAME, "expired": False, "size_in_bytes": len(data),
            "digest": "sha256:" + source.digest(data),
            "workflow_run": {"id": 123, "head_sha": WORKFLOW["commit"]}}


def environment(approved):
    return {"GITHUB_SHA": WORKFLOW["commit"], "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2",
            "GH_TOKEN": "read-secret", "AWS_SECRET_ACCESS_KEY": "unrelated-secret",
            "OWNED_TOOL_TARGET": TARGET, "OWNED_TOOL_SOURCE_JSON": json.dumps(approved),
            "GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main",
            "GITHUB_WORKFLOW_SHA": WORKFLOW["commit"],
            "GITHUB_WORKFLOW_REF": execution.REPOSITORY + "/" + execution.WORKFLOW_PATH + "@refs/heads/main",
            "GITHUB_WORKSPACE": "/unused/trusted-checkout"}


def execution_api():
    repository = {"id": 444, "full_name": execution.REPOSITORY, "default_branch": "main"}
    run = {"id": 123, "run_attempt": 2, "head_sha": WORKFLOW["commit"], "head_branch": "main",
           "event": "workflow_dispatch", "workflow_id": 555, "path": execution.WORKFLOW_PATH,
           "repository": {"id": 444, "full_name": execution.REPOSITORY},
           "head_repository": {"id": 444, "full_name": execution.REPOSITORY}}
    workflow = {"id": 555, "path": execution.WORKFLOW_PATH}
    return [json.dumps(item).encode() for item in (repository, run, workflow)]


class DownloadTests(unittest.TestCase):
    def test_same_run_read_only_admission_preserves_exact_bytes(self):
        approved, contents, _ = fixture()
        data = zip_data(contents)
        item = metadata(data)
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, environment(approved)), \
                patch.object(download, "gh", side_effect=execution_api() + [json.dumps([{"artifacts": [item]}]), data]) as gh:
            destination = Path(temporary) / "candidate"
            record = download.download(NAME, destination)
            self.assertEqual(record["artifact_id"], 987)
            self.assertEqual(record["api_digest"], item["digest"])
            self.assertEqual(record["workflow"], WORKFLOW)
            self.assertIsNone(record["behavioral_qualification"])
            for name, payload in contents.items():
                self.assertEqual((destination / name).read_bytes(), payload)
            self.assertTrue((destination / "artifact-admission.json").is_file())
            self.assertEqual(gh.call_args_list[0].args[0], execution.PREFIX)
            self.assertEqual(gh.call_args_list[1].args[0], execution.PREFIX + "/actions/runs/123")
            self.assertEqual(gh.call_args_list[2].args[0], execution.PREFIX + "/actions/workflows/555")
            self.assertEqual(gh.call_args_list[3].args[0], "repos/tailrocks/velnor-new/actions/runs/123/artifacts")
            self.assertTrue(gh.call_args_list[3].kwargs["paginate"])
            self.assertEqual(gh.call_args_list[4].args[0], "repos/tailrocks/velnor-new/actions/artifacts/987/zip")
            documents = {key: (destination / filename).read_bytes()
                         for key, filename in execution.API_EVIDENCE_FILES.items()}
            execution.validate_execution_evidence(record["execution"], documents)
            env = gh.call_args_list[0].args[1]
            self.assertEqual(env["GH_TOKEN"], "read-secret")
            self.assertNotIn("AWS_SECRET_ACCESS_KEY", env)

    def test_bad_name_or_existing_checkout_destination_fails_before_network(self):
        approved, _, _ = fixture()
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, environment(approved)), \
                patch.object(download, "gh") as gh:
            for name in (NAME.replace("-2-", "-1-"), NAME + "?url", "https://attacker/artifact"):
                with self.assertRaises(ValueError):
                    download.download(name, Path(temporary) / "candidate")
            with self.assertRaises(ValueError):
                download.download(NAME, Path(temporary))
            with self.assertRaises(ValueError):
                download.download(NAME, Path("/unused/trusted-checkout/candidate"))
            gh.assert_not_called()

    def test_api_identity_expiry_digest_size_and_duplicates_rejected(self):
        item = metadata(b"zip")
        for change in ({"expired": True}, {"id": True}, {"id": 0}, {"size_in_bytes": 0},
                       {"size_in_bytes": download.MAX_ZIP + 1}, {"digest": None},
                       {"digest": "sha256:" + "0" * 64},
                       {"workflow_run": {"id": 124, "head_sha": WORKFLOW["commit"]}},
                       {"workflow_run": {"id": 123, "head_sha": "e" * 40}}):
            with self.subTest(change=change), self.assertRaises(ValueError):
                download.select_artifact(json.dumps([{"artifacts": [{**item, **change}]}]), NAME, WORKFLOW)
        for pages in ([{"artifacts": []}], [{"artifacts": [item]}, {"artifacts": [item]}]):
            with self.assertRaises(ValueError):
                download.select_artifact(json.dumps(pages), NAME, WORKFLOW)

    def test_zip_digest_rejected_before_parsing_or_destination_creation(self):
        approved, _, _ = fixture()
        item = metadata(b"expected")
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, environment(approved)), \
                patch.object(download, "gh", side_effect=execution_api() + [json.dumps([{"artifacts": [item]}]), b"changed"]), \
                patch.object(download, "zip_members") as parse:
            destination = Path(temporary) / "candidate"
            with self.assertRaises(ValueError):
                download.download(NAME, destination)
            parse.assert_not_called()
            self.assertFalse(destination.exists())

    def test_actual_execution_api_mismatch_stops_before_artifact_api(self):
        approved, _, _ = fixture()
        api = execution_api()
        run = json.loads(api[1])
        run["head_repository"] = {"id": 999, "full_name": "fork/velnor-new"}
        api[1] = json.dumps(run).encode()
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, environment(approved)), \
                patch.object(download, "gh", side_effect=api) as gh:
            destination = Path(temporary) / "candidate"
            with self.assertRaises(ValueError):
                download.download(NAME, destination)
            self.assertEqual(gh.call_count, 2)
            self.assertFalse(destination.exists())

    def test_event_push_branch_is_bound_in_composed_download_evidence(self):
        approved, contents, _ = fixture()
        data = zip_data(contents)
        api = execution_api()
        run = json.loads(api[1])
        run.update(event="push", head_branch=execution.CANDIDATE_BRANCH)
        api[1] = json.dumps(run).encode()
        env = {**environment(approved), "GITHUB_EVENT_NAME": "push",
               "GITHUB_REF": "refs/heads/" + execution.CANDIDATE_BRANCH,
               "GITHUB_WORKFLOW_REF": execution.REPOSITORY + "/" + execution.WORKFLOW_PATH +
               "@refs/heads/" + execution.CANDIDATE_BRANCH}
        with tempfile.TemporaryDirectory() as temporary, patch.dict(os.environ, env), \
                patch.object(download, "gh", side_effect=api + [json.dumps([{"artifacts": [metadata(data)]}]), data]):
            record = download.download(NAME, Path(temporary) / "candidate")
            self.assertEqual(record["execution"]["event"], "push")
            self.assertEqual(record["execution"]["head_branch"], execution.CANDIDATE_BRANCH)

    def test_zip_rejects_escape_symlink_duplicate_case_and_expansion(self):
        for contents in ({"../escape": b"bad"}, {"a/file": b"bad"}, {"a\\file": b"bad"},
                         {"CASE": b"a", "case": b"b"}, {"noticé": b"bad"}):
            with self.subTest(contents=contents), self.assertRaises(ValueError):
                download.zip_members(zip_data(contents))
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            link = zipfile.ZipInfo("link")
            link.create_system, link.external_attr = 3, (stat.S_IFLNK | 0o777) << 16
            archive.writestr(link, "outside")
        with self.assertRaises(ValueError):
            download.zip_members(output.getvalue())
        with patch.object(download, "MAX_CONTENT", 2), self.assertRaises(ValueError):
            download.zip_members(zip_data({"regular": b"expanded"}))
        with patch.object(download, "MAX_ZIP", 2), self.assertRaises(ValueError):
            download.zip_members(zip_data({"regular": b"bounded"}))

    def test_receipt_requires_same_attempt_source_target_and_binary(self):
        approved, contents, receipt = fixture()
        for change in ({"target": "x86_64-unknown-linux-gnu"}, {"tool": "mbx"},
                       {"workflow": {**receipt["workflow"], "run_attempt": "1"}},
                       {"source": {**receipt["source"], "tree": "e" * 40}},
                       {"behavioral_qualification": {"passed": True}},
                       {"artifact": {**receipt["artifact"], "binary_sha256": "d" * 64}}):
            changed = {**contents, "candidate-receipt.json": json.dumps({**receipt, **change}).encode()}
            with self.subTest(change=change), self.assertRaises(ValueError):
                download.admit_contents(changed, approved, TARGET, WORKFLOW)
        changed = {**contents, "source-receipt.json": b"altered source receipt"}
        with self.assertRaises(ValueError):
            download.admit_contents(changed, approved, TARGET, WORKFLOW)

    def test_gh_read_arguments_stream_bound_and_no_token_arguments(self):
        process = MagicMock()
        process.stdout.read.return_value = b"response"
        process.wait.return_value = 0
        process.__enter__.return_value = process
        with patch.object(download.subprocess, "Popen", return_value=process) as popen:
            self.assertEqual(download.gh("repos/tailrocks/velnor-new/actions/runs/123/artifacts",
                                         {"GH_TOKEN": "read-secret"}, 100, paginate=True), b"response")
            self.assertEqual(popen.call_args.args[0], ["gh", "api",
                "repos/tailrocks/velnor-new/actions/runs/123/artifacts", "--paginate", "--slurp"])
            self.assertNotIn("read-secret", popen.call_args.args[0])
            process.stdout.read.return_value = b"too long"
            with self.assertRaises(ValueError):
                download.gh("repos/tailrocks/velnor-new/actions/artifacts/987/zip", {}, 2)
            process.kill.assert_called_once()
        with patch.object(download.subprocess, "Popen") as popen, self.assertRaises(ValueError):
            download.gh("https://attacker.example/artifact", {"GH_TOKEN": "read-secret"}, 2)
        popen.assert_not_called()


if __name__ == "__main__":
    unittest.main()

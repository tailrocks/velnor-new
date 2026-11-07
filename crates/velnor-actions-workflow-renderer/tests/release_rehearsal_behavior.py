# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SRC = Path(__file__).resolve().parents[1] / "src"
HELPERS = (SRC / "release_rehearsal_paths.py").read_text()


def module(name):
    namespace = {"__name__": name}
    text = (SRC / f"release_rehearsal_{name}.py").read_text()
    exec(compile(text.replace("__VELNOR_PATHS_PYTHON__", HELPERS), name, "exec"), namespace)
    return namespace


class RehearsalBehavior(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        self.addCleanup(self.temp.cleanup)

    def stage(self):
        subprocess.run(["git", "-C", str(self.repo), "add", "."], check=True)

    def test_exact_blobs_and_safe_symlinks(self):
        (self.repo / ".gitattributes").write_text("hidden export-ignore\n")
        (self.repo / "hidden").write_bytes(b"original")
        (self.repo / "link").symlink_to("hidden")
        self.stage()
        archive = self.root / "source.tar"
        module("archive")["build_archive"](self.repo, archive)
        out = self.root / "out"
        module("extract")["extract_archive"](archive, out)
        self.assertEqual((out / "hidden").read_bytes(), b"original")
        self.assertTrue((out / "link").is_symlink())
        self.assertEqual((out / "link").read_bytes(), b"original")
        (self.repo / "hidden").write_bytes(b"changed")
        with self.assertRaises(ValueError):
            module("archive")["build_archive"](self.repo, self.root / "changed.tar")

    def test_unsafe_archive_members_fail_closed(self):
        cases = [("../escape", None), ("link", "../outside"), ("cycle", "cycle")]
        for index, (name, target) in enumerate(cases):
            archive = self.root / f"bad-{index}.tar"
            with tarfile.open(archive, "w") as tar:
                entry = tarfile.TarInfo(name)
                if target is None:
                    entry.size = 1
                    tar.addfile(entry, io.BytesIO(b"x"))
                else:
                    entry.type = tarfile.SYMTYPE
                    entry.linkname = target
                    tar.addfile(entry)
            with self.assertRaises(ValueError):
                module("extract")["extract_archive"](archive, self.root / f"bad-{index}")

    def test_archive_bounds_and_hardlinks_fail_closed(self):
        large = self.repo / "large"
        with large.open("wb") as out:
            out.truncate(100 * 1024 * 1024 + 1)
        with self.assertRaises(ValueError):
            module("archive")["read_entry"](self.repo, ["large"], b"100644", 100 * 1024 * 1024)
        archive = self.root / "oversized.tar"
        with archive.open("wb") as out:
            out.truncate(100 * 1024 * 1024 + 1)
        with self.assertRaises(ValueError):
            module("extract")["extract_archive"](archive, self.root / "oversized")
        hardlink = self.root / "hardlink.tar"
        with tarfile.open(hardlink, "w") as tar:
            entry = tarfile.TarInfo("link")
            entry.type = tarfile.LNKTYPE
            entry.linkname = "target"
            tar.addfile(entry)
        with self.assertRaises(ValueError):
            module("extract")["extract_archive"](hardlink, self.root / "hardlink")

    def test_archive_rejects_oversized_git_index(self):
        namespace = module("archive")
        class GitProcess:
            def __init__(self, *_args, **_kwargs):
                self.stdout = io.BytesIO(b"x\0" * 100001)
            def wait(self):
                return 0
            def poll(self):
                return 0
        with patch.object(subprocess, "Popen", GitProcess):
            with self.assertRaisesRegex(ValueError, "source index is too large"):
                namespace["build_archive"](self.repo, self.root / "index.tar")

    def test_preparation_binds_index_tree_and_outputs_archive(self):
        (self.repo / "rehearse.sh").write_text("exit 0\n")
        self.stage()
        subprocess.run(["git", "-C", str(self.repo), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], check=True)
        sha = subprocess.check_output(["git", "-C", str(self.repo), "rev-parse", "HEAD"], text=True).strip()
        checkout = self.root / "policy"
        self.repo.rename(checkout)
        output = self.root / "github-output"
        rendered = os.environ.get("VELNOR_RENDERED_PREPARE")
        text = Path(rendered).read_text() if rendered else (SRC / "release_rehearsal_prepare.sh").read_text().replace(
            "__VELNOR_ARCHIVE_PYTHON__",
            (SRC / "release_rehearsal_archive.py").read_text().replace("__VELNOR_PATHS_PYTHON__", HELPERS),
        )
        env = dict(os.environ, RUNNER_TEMP=str(self.root), GITHUB_OUTPUT=str(output), GITHUB_SHA=sha,
                   CANDIDATE_PR="1", CANDIDATE_SHA="a" * 40, VELNOR_REHEARSAL_SCRIPT="rehearse.sh")
        for filename, setup in [("untracked", None), ("ignored", "ignored\n")]:
            exclude = checkout / ".git" / "info" / "exclude"
            original = exclude.read_text()
            if setup:
                exclude.write_text(original + setup)
            (checkout / filename).write_text("untrusted")
            failed = subprocess.run(["bash", "-c", text], cwd=self.root, env=env, capture_output=True)
            self.assertNotEqual(failed.returncode, 0, filename)
            self.assertFalse((self.root / "rehearsal-source.tar").exists())
            (checkout / filename).unlink()
            exclude.write_text(original)
        subprocess.run(["bash", "-c", text], cwd=self.root, env=env, check=True)
        archive = self.root / "rehearsal-source.tar"
        self.assertIn(hashlib.sha256(archive.read_bytes()).hexdigest(), output.read_text())
        (checkout / "rehearse.sh").write_text("exit 1\n")
        subprocess.run(["git", "-C", str(checkout), "add", "."], check=True)
        archive.unlink()
        self.assertNotEqual(subprocess.run(["bash", "-c", text], cwd=self.root, env=env, capture_output=True).returncode, 0)

    def test_executor_rechecks_digest_and_scrubs_environment(self):
        (self.repo / "rehearse.sh").write_text(
            'test -z "${GH_TOKEN+x}${ACTIONS_RUNTIME_TOKEN+x}${CANARY+x}"\n'
            'printf ok > "$RUNNER_TEMP/result"\n'
        )
        self.stage()
        subprocess.run(["git", "-C", str(self.repo), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], check=True)
        sha = subprocess.check_output(["git", "-C", str(self.repo), "rev-parse", "HEAD"], text=True).strip()
        upload = self.root / "rehearsal"
        upload.mkdir()
        archive = upload / "rehearsal-source.tar"
        module("archive")["build_archive"](self.repo, archive)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        bundle = upload / "rehearsal-source.bundle"
        subprocess.run(["git", "-C", str(self.repo), "bundle", "create", str(bundle), "HEAD"], check=True)
        bundle_digest = hashlib.sha256(bundle.read_bytes()).hexdigest()
        rendered = os.environ.get("VELNOR_RENDERED_EXECUTE")
        text = Path(rendered).read_text() if rendered else (SRC / "release_rehearsal_execute.sh").read_text().replace(
            "__VELNOR_EXTRACT_PYTHON__",
            (SRC / "release_rehearsal_extract.py").read_text().replace("__VELNOR_PATHS_PYTHON__", HELPERS),
        ).replace(
            "__VELNOR_ARCHIVE_PYTHON__",
            (SRC / "release_rehearsal_archive.py").read_text().replace("__VELNOR_PATHS_PYTHON__", HELPERS),
        ).replace("__VELNOR_OUTPUT_ENV__", "")
        env = dict(os.environ, RUNNER_TEMP=str(self.root), VELNOR_SOURCE_SHA256=digest,
                   CANDIDATE_PR="1", CANDIDATE_SHA="a" * 40, GITHUB_SHA=sha, VELNOR_BUNDLE_SHA256=bundle_digest,
                   VELNOR_REHEARSAL_SCRIPT="rehearse.sh", GH_TOKEN="fixture", CANARY="fixture",
                   GIT_DIR="/invalid/injected", GIT_CONFIG_COUNT="1",
                   GIT_CONFIG_KEY_0="core.hooksPath", GIT_CONFIG_VALUE_0="/invalid/injected")
        subprocess.run(["bash", "-c", text], env=env, check=True)
        self.assertEqual((self.root / "result").read_text(), "ok")
        for kind in ("bundle", "sha"):
            with tempfile.TemporaryDirectory() as another:
                another_root = Path(another)
                another_upload = another_root / "rehearsal"
                another_upload.mkdir()
                (another_upload / "rehearsal-source.tar").write_bytes(archive.read_bytes())
                (another_upload / "rehearsal-source.bundle").write_bytes(bundle.read_bytes() + (b"x" if kind == "bundle" else b""))
                changed = dict(env, RUNNER_TEMP=another, GITHUB_SHA=("b" * 40 if kind == "sha" else sha))
                failed = subprocess.run(["bash", "-c", text], env=changed, capture_output=True)
                self.assertNotEqual(failed.returncode, 0, kind)
                self.assertFalse((another_root / "result").exists())
        with tempfile.TemporaryDirectory() as second:
            second_root = Path(second)
            second_upload = second_root / "rehearsal"
            second_upload.mkdir()
            (second_upload / "rehearsal-source.tar").write_bytes(archive.read_bytes() + b"x")
            env["RUNNER_TEMP"] = second
            failed = subprocess.run(["bash", "-c", text], env=env, capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn("FAILED", failed.stdout)
            self.assertFalse((second_root / "result").exists())

    def test_witness_records_only_unverified_candidate_inputs(self):
        script = os.environ.get("VELNOR_RENDERED_WITNESS", str(SRC / "release_rehearsal_witness.sh"))
        env = dict(os.environ, RUNNER_TEMP=str(self.root), GITHUB_REPOSITORY="owner/repo",
                   GITHUB_SHA="a" * 40, GITHUB_RUN_ID="7", GITHUB_RUN_ATTEMPT="1",
                   VELNOR_SOURCE_SHA256="b" * 64, CANDIDATE_PR="8", CANDIDATE_SHA="c" * 40,
                   VELNOR_REHEARSAL_SCRIPT="qualification/rehearse.sh",
                   VELNOR_REHEARSAL_ENVIRONMENT="qualification", VELNOR_REVIEWER_ID="42")
        subprocess.run(["bash", script], env=env, check=True)
        evidence = json.loads((self.root / "rehearsal-observation.json").read_text())
        self.assertEqual(evidence["observation"], "script-exited-successfully")
        self.assertEqual(evidence["unverified_candidate_sha"], "c" * 40)
        self.assertNotIn("qualified", evidence)
        for key, value in [("GITHUB_RUN_ATTEMPT", "2"), ("CANDIDATE_SHA", "not-a-sha")]:
            bad = dict(env, **{key: value})
            self.assertNotEqual(subprocess.run(["bash", script], env=bad, capture_output=True).returncode, 0)

    def test_artifact_marker_is_bound_to_id_digest_and_attempt(self):
        script = os.environ.get("VELNOR_RENDERED_MARKER", str(SRC / "release_rehearsal_artifact_marker.sh"))
        env = dict(os.environ, GITHUB_RUN_ID="7", GITHUB_RUN_ATTEMPT="1",
                   VELNOR_ARTIFACT_ID="123", VELNOR_ARTIFACT_DIGEST="a" * 64)
        result = subprocess.run(["bash", script], env=env, check=True, capture_output=True, text=True)
        self.assertEqual(result.stdout, f"VELNOR_REHEARSAL_WITNESS run=7 attempt=1 artifact=123 sha256={'a' * 64}\n")
        for key, value in [("VELNOR_ARTIFACT_ID", "0"), ("VELNOR_ARTIFACT_DIGEST", "invalid"), ("GITHUB_RUN_ATTEMPT", "2")]:
            self.assertNotEqual(subprocess.run(["bash", script], env=dict(env, **{key: value}), capture_output=True).returncode, 0)

    def test_policy_requires_human_approval_and_rejects_bypass(self):
        fake = self.root / "bin"
        fake.mkdir()
        curl = fake / "curl"
        curl.write_text('#!/bin/sh\nfor arg do url="$arg"; done\ncase "$url" in */approvals) printf "%s" "$REVIEWS";; */branches/*) printf "%s" "$BRANCH";; *) printf "%s" "$POLICY";; esac\n')
        curl.chmod(0o755)
        sha = "a" * 40
        policy = {"id": 17, "name": "qualification", "can_admins_bypass": False,
                  "deployment_branch_policy": {"protected_branches": True, "custom_branch_policies": False},
                  "protection_rules": [{"type": "required_reviewers", "reviewers": [{"type": "User", "reviewer": {"type": "User", "id": 42}}]}]}
        review = [{"state": "approved", "user": {"type": "User", "id": 42},
                   "environments": [{"name": "qualification", "id": 17}]}]
        env = dict(os.environ, PATH=f"{fake}:/usr/bin:/bin:/opt/homebrew/bin", GH_TOKEN="fixture",
                   GITHUB_REPOSITORY="owner/repo", GITHUB_SHA=sha, GITHUB_RUN_ID="1",
                   GITHUB_RUN_ATTEMPT="1", GITHUB_REF="refs/heads/main", GITHUB_EVENT_NAME="workflow_dispatch",
                   VELNOR_DEFAULT_BRANCH="main", VELNOR_REHEARSAL_ENVIRONMENT="qualification",
                   VELNOR_REVIEWER_ID="42", VELNOR_REHEARSAL_STAGE="approved", POLICY=json.dumps(policy),
                   BRANCH=json.dumps({"protected": True, "commit": {"sha": sha}}), REVIEWS=json.dumps(review))
        script = os.environ.get("VELNOR_RENDERED_POLICY", str(SRC / "release_rehearsal_policy.sh"))
        self.assertEqual(subprocess.run(["bash", script], env=env, capture_output=True).returncode, 0)
        policy["can_admins_bypass"] = True
        env["POLICY"] = json.dumps(policy)
        self.assertNotEqual(subprocess.run(["bash", script], env=env, capture_output=True).returncode, 0)
        policy["can_admins_bypass"] = False
        env["POLICY"] = json.dumps(policy)
        review[0]["state"] = "bypassed"
        env["REVIEWS"] = json.dumps(review)
        self.assertNotEqual(subprocess.run(["bash", script], env=env, capture_output=True).returncode, 0)


if __name__ == "__main__":
    unittest.main()

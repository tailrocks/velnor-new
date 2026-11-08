# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import hashlib
import http.server
import io
import json
import os
import pathlib
import signal
import struct
import subprocess
import shutil
import tomllib
import re
import sys
import tarfile
import urllib.parse

from fixture_support import approved_baseline, index_path

root = pathlib.Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=True)
source = pathlib.Path(os.environ["RUNNER_TEMP"]) / "velnor-crate-release-source"


def run(*args, cwd):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(data)


repository = "owner/project"
package = "example"
version = "0.1.1"
baseline_version = "0.1.0"
reviewer_id = 139017
required_workflow = ".github/workflows/ci.yml"
multi_manifest = os.environ.get("VELNOR_FIXTURE_MULTI_MANIFEST")
multi_entries = json.loads(pathlib.Path(multi_manifest).read_text())["archives"] if multi_manifest else []
multi_names = {entry["name"] for entry in multi_entries}
deferred = os.environ.get("VELNOR_FIXTURE_FAILURE") == "coordinate-evidence-deferred"
deferred_a = "example-evidence-a"
deferred_b = "example-evidence-b"
deferred_baselines = {}
supplied = os.environ.get("VELNOR_FIXTURE_SUPPLIED_INTENT")
supplied_intent = None
dependency_snapshots = {}
alternate_baseline = None
if deferred:
    package = deferred_b
    for name in (deferred_a, deferred_b):
        baseline = root / ("baseline-" + name)
        baseline.mkdir()
        write(baseline / "Cargo.toml", f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2021"\nlicense="MIT"\ndescription="evidence fixture"\n')
        write(baseline / "src/lib.rs", "pub fn example() -> u32 { 1 }\n")
        run("cargo", "generate-lockfile", "--offline", cwd=baseline)
        run("cargo", "package", "--offline", "--no-verify", "--allow-dirty", cwd=baseline)
        data = (baseline / f"target/package/{name}-0.1.0.crate").read_bytes()
        deferred_baselines[name] = (data, hashlib.sha256(data).hexdigest())
    archive, checksum = deferred_baselines[deferred_b]
    source.mkdir()
    write(source / ".gitignore", "/target/\n")
    write(source / "Cargo.toml", '[workspace]\nmembers=["a","b"]\nresolver="2"\n')
    for directory, name in (("a", deferred_a), ("b", deferred_b)):
        dependency = f'[dependencies]\n{deferred_a}={{path="../a",version="=0.1.1"}}\n' if directory == "b" else ""
        write(source / directory / "Cargo.toml", f'[package]\nname="{name}"\nversion="0.1.1"\nedition="2021"\nlicense="MIT"\ndescription="evidence fixture"\n{dependency}')
        write(source / directory / "src/lib.rs", "pub fn example() -> u32 { 1 }\n")
    run("cargo", "generate-lockfile", "--offline", cwd=source)
    run("git", "init", "-q", cwd=source)
    run("git", "remote", "add", "origin", "https://github.com/" + repository, cwd=source)
    run("git", "add", ".", cwd=source)
    run("git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-qm", "workspace source", cwd=source)
    source_sha = run("git", "rev-parse", "HEAD", cwd=source)
elif multi_entries:
    original = pathlib.Path(os.environ["VELNOR_FIXTURE_MULTI_SOURCE"]).resolve(strict=True)
    package = multi_entries[0]["name"]
    version = "0.1.0"
    baseline_version = version
    run("git", "clone", "--quiet", "--no-local", str(original), str(source), cwd=root)
    run("git", "remote", "set-url", "origin", "https://github.com/" + repository, cwd=source)
    source_sha = run("git", "rev-parse", "HEAD", cwd=source)
    archive = pathlib.Path(multi_entries[0]["path"]).read_bytes()
    checksum = hashlib.sha256(archive).hexdigest()
elif supplied:
    if os.environ.get("VELNOR_FIXTURE_FAILURE") in ("source", "checksum", "verify-source"):
        raise ValueError("supplied fixture does not support source, checksum, or verify-source fault injection")
    supplied_intent = pathlib.Path(supplied).read_text()
    selected = tomllib.loads(supplied_intent)
    repository = selected["repository"]
    package = selected["recovery"]["package"]
    version = selected["recovery"]["version"]
    reviewer_id = selected["trusted_reviewer_id"]
    required_workflow = selected["required_workflow_path"]
    assert selected["packages"] == [package]
    assert selected["default_branch"] == "main" and selected["environment"] == "release"
    assert selected["registry"] == "crates-io" and selected["manifest_path"] == "Cargo.toml"
    original = pathlib.Path(os.environ["VELNOR_FIXTURE_SUPPLIED_SOURCE"]).resolve(strict=True)
    assert run("git", "-c", f"safe.directory={original}", "rev-parse", "HEAD", cwd=original) == selected["recovery"]["source_sha"]
    assert not run("git", "-c", f"safe.directory={original}", "status", "--porcelain", cwd=original)
    run("git", "clone", "--quiet", "--no-local", str(original), str(source), cwd=root)
    run("git", "remote", "set-url", "origin", "https://github.com/" + repository, cwd=source)
    source_sha = run("git", "rev-parse", "HEAD", cwd=source)
    assert source_sha == selected["recovery"]["source_sha"]
    metadata = json.loads(run("cargo", "metadata", "--offline", "--locked", "--format-version", "1", cwd=source))
    selected_package = next(item for item in metadata["packages"] if item["name"] == package and item["version"] == version)
    snapshot_dir = pathlib.Path(os.environ["VELNOR_FIXTURE_DEPENDENCY_SNAPSHOTS"]).resolve(strict=True)
    for dependency in selected_package["dependencies"]:
        name = dependency["name"]
        if name == package or name in dependency_snapshots:
            continue
        assert re.fullmatch(r"[a-zA-Z0-9_-]{1,64}", name)
        payload = json.loads((snapshot_dir / (name + ".json")).read_bytes())
        assert payload["crate"]["name"] == name and payload["crate"]["id"] == name
        dependency_snapshots[name] = payload
    baseline_version = os.environ["VELNOR_FIXTURE_BASELINE_VERSION"]
    archive = pathlib.Path(os.environ["VELNOR_FIXTURE_BASELINE_ARCHIVE"]).read_bytes()
    checksum = hashlib.sha256(archive).hexdigest()
    index = [json.loads(line) for line in pathlib.Path(os.environ["VELNOR_FIXTURE_BASELINE_INDEX"]).read_text().splitlines()]
    approved_baseline(index, package, baseline_version, checksum)
else:
    source.mkdir()
    manifest = '[package]\nname = "example"\nversion = "{}"\nedition = "2021"\nlicense = "MIT"\ndescription = "Generic release rehearsal"\nrepository = "https://github.com/owner/project"\n'
    if os.environ.get("VELNOR_FIXTURE_FAILURE") == "coordinate-evidence-drift":
        baseline_version = "0.0.1"
    write(source / "Cargo.toml", manifest.format(baseline_version))
    write(source / "src/lib.rs", "pub fn example() -> u32 { 1 }\n")
    run("cargo", "generate-lockfile", "--offline", cwd=source)
    run("cargo", "package", "--offline", "--no-verify", "--allow-dirty", cwd=source)
    archive = (source / f"target/package/example-{baseline_version}.crate").read_bytes()
    checksum = hashlib.sha256(archive).hexdigest()
    failure = os.environ.get("VELNOR_FIXTURE_FAILURE")
    if failure in ("coordinate-evidence-drift", "coordinate-evidence-checksum-drift"):
        alternate_version = "0.1.0" if failure == "coordinate-evidence-drift" else baseline_version
        write(source / "Cargo.toml", manifest.format(alternate_version))
        if failure == "coordinate-evidence-checksum-drift":
            write(source / "src/lib.rs", "pub fn example() -> u32 { 2 }\n")
        run("cargo", "generate-lockfile", "--offline", cwd=source)
        run("cargo", "package", "--offline", "--no-verify", "--allow-dirty", cwd=source)
        alternate_archive = (source / f"target/package/example-{alternate_version}.crate").read_bytes()
        alternate_baseline = {
            "version": alternate_version,
            "archive": alternate_archive,
            "checksum": hashlib.sha256(alternate_archive).hexdigest(),
            "id": 102 if alternate_version != baseline_version else 100,
        }
        assert alternate_baseline["checksum"] != checksum
        write(source / "src/lib.rs", "pub fn example() -> u32 { 1 }\n")
    if os.environ.get("VELNOR_FIXTURE_FAILURE") == "checksum":
        checksum = "0" * 64
    write(source / "Cargo.toml", manifest.format("0.1.1"))
    run("cargo", "generate-lockfile", "--offline", cwd=source)
    run("git", "init", "-q", cwd=source)
    run("git", "remote", "add", "origin", "https://github.com/owner/project", cwd=source)
    run("git", "add", "Cargo.toml", "Cargo.lock", "src/lib.rs", cwd=source)
    run(
        "git",
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "source",
        cwd=source,
    )
    source_sha = run("git", "rev-parse", "HEAD", cwd=source)
    if os.environ.get("VELNOR_FIXTURE_FAILURE") == "source":
        write(source / "src/lib.rs", "pub fn example() -> u32 { 2 }\n")
    shutil.rmtree(source / "target")
assets = os.environ.get("VELNOR_FIXTURE_OFFICIAL_ASSETS")
generator_version = "0.1.5"
generator_source = "f1041f322c54cd7edfdef06afd307799eeaa801a"
generator_manifest_sha = (
    "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52"
)
if assets:
    manifest_bytes = (pathlib.Path(assets) / "release-manifest.json").read_bytes()
    generator = json.loads(manifest_bytes)
    generator_version = generator["version"]
    generator_source = generator["commit"]
    generator_manifest_sha = hashlib.sha256(manifest_bytes).hexdigest()
    (root / ".velnor").mkdir()
    (root / ".velnor/release-manifest.json").write_bytes(manifest_bytes)
authentication = "bootstrap-token" if os.environ.get("VELNOR_FIXTURE_FAILURE") == "coordinate-missing-bootstrap" else "trusted-publishing"
intent = f'''schema = 1
repository = "owner/project"
default_branch = "main"
environment = "release"
required_workflow_path = ".github/workflows/ci.yml"
trusted_reviewer_id = 139017
rust_version = "1.99.1"
generator_version = "{generator_version}"
generator_source_sha = "{generator_source}"
generator_manifest_sha256 = "{generator_manifest_sha}"
release_plz_version = "0.4.3"
runner = "ubuntu-26.04"
authentication = "{authentication}"
manifest_path = "Cargo.toml"
pr_write_paths = ["CHANGELOG.md", "Cargo.lock", "Cargo.toml"]
registry = "crates-io"
packages = ["example"]
tag_template = "{{{{ package }}}}-v{{{{ version }}}}"
[publish_features]
example = []
[recovery]
package = "example"
version = "0.1.1"
source_sha = "{source_sha}"
'''
if deferred:
    intent = re.sub(r'^packages = .*$', 'packages = ' + json.dumps([deferred_a, deferred_b]), intent, flags=re.MULTILINE)
    intent = re.sub(r'^pr_write_paths = .*$', 'pr_write_paths = ' + json.dumps(sorted(["Cargo.lock", "Cargo.toml", "CHANGELOG.md", "a/Cargo.toml", "b/Cargo.toml"])), intent, flags=re.MULTILINE)
    intent = intent.split("[publish_features]", 1)[0] + f'[publish_features]\n{deferred_a} = []\n{deferred_b} = []\n'
elif multi_entries:
    names = sorted(multi_names)
    intent = re.sub(r'^packages = .*$', 'packages = ' + json.dumps(names), intent, flags=re.MULTILINE)
    paths = ["Cargo.lock", "Cargo.toml", "CHANGELOG.md"]
    paths.extend(path for name in names for path in (f"{name}/Cargo.toml", f"{name}/CHANGELOG.md"))
    intent = re.sub(r'^pr_write_paths = .*$', 'pr_write_paths = ' + json.dumps(sorted(paths)), intent, flags=re.MULTILINE)
    intent = intent.split("[publish_features]", 1)[0] + "[publish_features]\n" + "".join(f'{name} = []\n' for name in names)
elif supplied_intent is not None:
    values = {
        "generator_version": generator_version,
        "generator_source_sha": generator_source,
        "generator_manifest_sha256": generator_manifest_sha,
    }
    intent = supplied_intent
    for key, value in values.items():
        intent, count = re.subn(rf'^({key} = )"[^"\n]*"$', rf'\g<1>"{value}"', intent, flags=re.MULTILINE)
        assert count == 1
    original = tomllib.loads(supplied_intent)
    rewritten = tomllib.loads(intent)
    for key in values:
        original.pop(key)
        rewritten.pop(key)
    assert original == rewritten
write(root / ".github/velnor-release-intent.toml", intent)
write(
    root / ".github/workflows/release.yml",
    f"name: release\nconcurrency:\n  group: release-crates-io-{repository}\n  cancel-in-progress: false\n  queue: max\njobs: {{}}\n",
)
run("git", "init", "-q", cwd=root)
run("git", "add", ".github", cwd=root)
if assets:
    run("git", "add", ".velnor/release-manifest.json", cwd=root)
run(
    "git",
    "-c",
    "user.name=Fixture",
    "-c",
    "user.email=fixture@example.invalid",
    "-c",
    "commit.gpgsign=false",
    "commit",
    "-qm",
    "intent",
    cwd=root,
)
policy = run("git", "rev-parse", "HEAD", cwd=root)
prior_policy = policy
if deferred:
    shutil.copytree(root / ".github", source / ".github", dirs_exist_ok=True)
    if assets:
        shutil.copytree(root / ".velnor", source / ".velnor", dirs_exist_ok=True)
    run("git", "add", ".github", cwd=source)
    if assets:
        run("git", "add", ".velnor/release-manifest.json", cwd=source)
    run("git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-qm", "release intent", cwd=source)
    source_sha = run("git", "rev-parse", "HEAD", cwd=source)
    policy = prior_policy = source_sha
    run("git", "fetch", "--quiet", "--no-tags", str(source), source_sha, cwd=root)
    run("git", "update-ref", "HEAD", source_sha, cwd=root)
    run("cargo", "package", "--offline", "--locked", "--no-verify", "--target-dir", str(root / "fixture-target"), "--package", deferred_a, cwd=source)
    deferred_a_archive = (root / f"fixture-target/package/{deferred_a}-0.1.1.crate").read_bytes()
    deferred_a_checksum = hashlib.sha256(deferred_a_archive).hexdigest()
if multi_entries:
    shutil.copytree(root / ".github", source / ".github", dirs_exist_ok=True)
    if assets:
        shutil.copytree(root / ".velnor", source / ".velnor", dirs_exist_ok=True)
    run("git", "add", ".github", cwd=source)
    if assets:
        run("git", "add", ".velnor/release-manifest.json", cwd=source)
    run("git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
        "-c", "commit.gpgsign=false", "commit", "-qm", "release intent", cwd=source)
    source_sha = run("git", "rev-parse", "HEAD", cwd=source)
    policy = source_sha
    run("git", "fetch", "--quiet", "--no-tags", str(source), source_sha, cwd=root)
    run("git", "update-ref", "HEAD", source_sha, cwd=root)
    for entry in multi_entries:
        run("cargo", "package", "--offline", "--locked", "--no-verify", "--target-dir", str(root / "fixture-target"), "--package", entry["name"], cwd=source)
        path = root / "fixture-target/package" / f'{entry["name"]}-0.1.0.crate'
        entry["path"] = str(path)
        entry["size"] = path.stat().st_size
        if not 0 < entry["size"] < 10_000_000:
            raise ValueError("invalid package archive size")
    if sum(entry["size"] for entry in multi_entries) <= 512 * 1024 * 1024:
        raise ValueError("fixture does not exceed the completion limit")
    pathlib.Path(os.environ["VELNOR_FIXTURE_FINAL_MULTI_MANIFEST"]).write_text(
        json.dumps({"source": str(source), "archives": multi_entries}))

mutations = 0
blobs = {}
tags = {}
refs = {f"refs/tags/{name}-v0.1.0": source_sha for name in multi_names}
published = None
published_features = {}
release = None
releases = {
    f"{name}-v0.1.0": {"id": index + 100, "tag_name": f"{name}-v0.1.0", "draft": False,
                          "prerelease": False,
                          "html_url": f"https://github.com/{repository}/releases/tag/{name}-v0.1.0"}
    for index, name in enumerate(sorted(multi_names))
}
uploads = 0
upload_attempts = 0


def active_alternate():
    marker = os.environ.get("VELNOR_NATIVE_PID_FILE")
    return alternate_baseline if alternate_baseline and marker and pathlib.Path(marker).is_file() else None


def deferred_a_visible():
    marker = os.environ.get("VELNOR_NATIVE_PID_FILE")
    return deferred and marker and pathlib.Path(marker).is_file()


def git_hash(kind, data):
    return hashlib.sha1(f"{kind} {len(data)}\0".encode() + data).hexdigest()


class Handler(http.server.BaseHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Connection", "close")
        self.close_connection = True
        super().end_headers()

    def log_message(self, fmt, *args):
        sys.stderr.write("fixture: " + fmt % args + "\n")

    def do_GET(self):
        p = self.path
        sha = policy if policy in p else prior_policy if prior_policy in p else source_sha
        status = 200
        obj = None
        if p == "/__fixture/state":
            obj = {
                "mutations": mutations,
                "refs": refs,
                "blobs": len(blobs),
                "tags": len(tags),
                "records": [json.loads(raw) for raw in blobs.values()],
                "uploads": uploads,
                "upload_attempts": upload_attempts,
                "published_sha256": hashlib.sha256(published).hexdigest()
                if published
                else None,
                "release": release,
                "releases": releases,
                "baseline_switched": active_alternate() is not None,
            }
        elif p.startswith(f"/repos/{repository}/git/matching-refs/tags/"):
            prefix = "refs/tags/" + urllib.parse.unquote(
                p.split("/git/matching-refs/tags/", 1)[1]
            )
            obj = [
                {"ref": name, "object": {"type": "tag", "sha": sha}}
                for name, sha in sorted(refs.items())
                if name.startswith(prefix)
            ]
        elif p.startswith(f"/repos/{repository}/git/ref/tags/velnor-receipt"):
            name = "refs/tags/" + urllib.parse.unquote(p.split("/git/ref/tags/", 1)[1])
            if name in refs:
                obj = {"ref": name, "object": {"type": "tag", "sha": refs[name]}}
            else:
                status, obj = 404, {"message": "Not Found"}
        elif p.startswith(f"/repos/{repository}/git/tags/"):
            sha = p.rsplit("/", 1)[-1]
            if sha in tags:
                obj = tags[sha]
            else:
                status, obj = 404, {"message": "Not Found"}
        elif p.startswith(f"/repos/{repository}/git/blobs/"):
            sha = p.rsplit("/", 1)[-1]
            if sha in blobs:
                raw = blobs[sha]
                obj = {
                    "sha": sha,
                    "encoding": "base64",
                    "content": base64.b64encode(raw).decode(),
                    "size": len(raw),
                }
            else:
                status, obj = 404, {"message": "Not Found"}
        elif p == f"/repos/{repository}":
            obj = {"id": 22, "full_name": repository, "default_branch": "main", "owner": {"id": 77, "login": repository.split("/", 1)[0]}}
        elif p == f"/repos/{repository}/git/ref/heads/main":
            obj = {
                "ref": "refs/heads/main",
                "object": {"type": "commit", "sha": policy},
            }
        elif p.startswith(f"/repos/{repository}/actions/runs/12/approvals") or p.startswith(f"/repos/{repository}/actions/runs/13/approvals"):
            approval_run = 13 if "/runs/13/" in p else 12
            approval_policy = policy if approval_run == 13 else prior_policy
            obj = [
                {
                    "state": "approved",
                    "user": {"type": "User", "id": reviewer_id},
                    "environments": [{"id": 9, "name": "release"}],
                    "comment": f"I reviewed the complete release policy at {approval_policy} and source {source_sha}, and authorize this source for run {approval_run}.",
                }
            ]
            if os.environ.get("VELNOR_FIXTURE_FAILURE") == "approval":
                obj[0]["comment"] = "Unrelated approval"
        elif p in (f"/repos/{repository}/actions/runs/12",
                   f"/repos/{repository}/actions/runs/12/attempts/1",
                   f"/repos/{repository}/actions/runs/13",
                   f"/repos/{repository}/actions/runs/13/attempts/1"):
            requested_run = 13 if "/runs/13" in p else 12
            obj = {
                "id": requested_run,
                "status": "completed",
                "run_attempt": 1,
                "head_sha": policy if requested_run == 13 else prior_policy,
                "head_branch": "main",
                "event": "push" if multi_names or deferred else "workflow_dispatch",
                "path": ".github/workflows/release.yml",
                "repository": {"id": 22, "full_name": repository, "owner": {"id": 77, "login": repository.split("/", 1)[0]}},
                "head_repository": {"id": 22, "full_name": repository, "owner": {"id": 77, "login": repository.split("/", 1)[0]}},
            }
        elif (multi_names or deferred) and "/commits/" in p and "/pulls?" in p:
            obj = [{"number": 17, "merged_at": "2026-11-03T16:00:00Z", "merge_commit_sha": source_sha,
                    "base": {"ref": "main", "repo": {"full_name": repository, "owner": {"id": 77, "login": repository.split("/", 1)[0]}}},
                    "head": {"ref": "release-plz-1", "sha": source_sha, "repo": {"full_name": repository, "owner": {"id": 77, "login": repository.split("/", 1)[0]}}},
                    "user": {"id": 54, "login": "release-bot"}}]
        elif (multi_names or deferred) and "/pulls/17/reviews?" in p:
            obj = [{"id": 1, "user": {"id": reviewer_id, "login": "owner", "type": "User"}, "state": "APPROVED",
                    "commit_id": source_sha, "author_association": "OWNER", "submitted_at": "2026-11-03T15:50:00Z"}]
        elif (multi_names or deferred) and "/git/commits/" in p:
            obj = {"sha": p.rsplit("/", 1)[-1], "tree": {"sha": "a" * 40}}
        elif (multi_names or deferred) and p.endswith("/collaborators/owner/permission"):
            obj = {"user": {"id": reviewer_id, "type": "User"}, "permission": "admin"}
        elif "/compare/" in p:
            obj = {"status": "ahead"}
        elif p.startswith(
            f"/repos/{repository}/environments/release/deployment-branch-policies?"
        ):
            obj = {
                "total_count": 1,
                "branch_policies": [{"type": "branch", "name": "main"}],
            }
        elif p == f"/repos/{repository}/environments/release":
            obj = {
                "id": 9,
                "name": "release",
                "can_admins_bypass": False,
                "deployment_branch_policy": {"custom_branch_policies": True},
                "protection_rules": [
                    {
                        "type": "required_reviewers",
                        "reviewers": [{"type": "User", "reviewer": {"id": reviewer_id}}],
                    }
                ],
            }
        elif "/check-runs?" in p:
            obj = {
                "total_count": 1,
                "check_runs": [
                    {
                        "id": 1,
                        "name": "Required",
                        "head_sha": sha,
                        "check_suite": {"id": 123},
                        "app": {"id": 15368, "slug": "github-actions"},
                        "status": "completed",
                        "conclusion": "success",
                        "started_at": "2026-11-03T15:40:00Z",
                    }
                ],
            }
        elif "/actions/runs?" in p:
            obj = {
                "total_count": 1,
                "workflow_runs": [
                    {
                        "id": 123,
                        "run_number": 1,
                        "run_attempt": 1,
                        "check_suite_id": 123,
                        "head_sha": sha,
                        "event": "push",
                        "head_branch": "main",
                        "repository": {"id": 22, "full_name": repository, "owner": {"id": 77, "login": repository.split("/", 1)[0]}},
                        "head_repository": {"id": 22, "full_name": repository, "owner": {"id": 77, "login": repository.split("/", 1)[0]}},
                        "path": required_workflow,
                        "status": "completed",
                        "conclusion": "success",
                    }
                ],
            }
        elif p.startswith(f"/repos/{repository}/actions/runs/123/attempts/1/jobs?"):
            obj = {
                "total_count": 1,
                "jobs": [{"id": 1, "name": "Required", "run_id": 123,
                          "run_attempt": 1, "status": "completed", "conclusion": "success",
                          "check_run_url": f"https://api.github.com/repos/{repository}/check-runs/1"}],
            }
        elif p.startswith(f"/repos/{repository}/rulesets?"):
            obj = [{"id": 1}]
        elif p == f"/repos/{repository}/rulesets/1":
            obj = {
                "id": 1,
                "target": "tag",
                "enforcement": "active",
                "bypass_actors": [],
                "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}},
                "rules": [{"type": "update"}, {"type": "deletion"}],
            }
        elif p.startswith(f"/repos/{repository}/git/ref/tags/"):
            name = "refs/tags/" + urllib.parse.unquote(p.split("/git/ref/tags/", 1)[1])
            if name in refs:
                obj = {"ref": name, "object": {"type": "commit", "sha": refs[name]}}
            else:
                status, obj = 404, {"message": "Not Found"}
        elif p.startswith(f"/repos/{repository}/releases/tags/"):
            requested_tag = urllib.parse.unquote(p.rsplit("/", 1)[-1])
            found = releases.get(requested_tag) if multi_names or deferred else release
            if found is None:
                status, obj = 404, {"message": "Not Found"}
            else:
                obj = found
        elif p == "/users/owner":
            obj = {"id": reviewer_id, "login": "owner", "type": "User"}
        elif p == "/api/v1/users/owner?include=linked_accounts":
            obj = {"user": {"id": reviewer_id, "login": "owner"}, "linked_accounts": [{"provider": "github", "account_id": str(reviewer_id), "login": "owner"}]}
        elif deferred and p == f"/api/v1/crates/{deferred_a}":
            entries = [{"id": 100, "crate": deferred_a, "num": "0.1.0", "yanked": False}]
            if deferred_a_visible():
                entries.append({"id": 101, "crate": deferred_a, "num": "0.1.1", "yanked": False})
            obj = {"crate": {"id": deferred_a, "name": deferred_a, "versions": [item["id"] for item in entries]}, "versions": entries}
        elif deferred and p == f"/api/v1/crates/{deferred_a}/owners":
            obj = {"users": [{"id": reviewer_id, "login": "owner", "kind": "user"}]}
        elif deferred and p == f"/api/v1/crates/{deferred_a}/0.1.0":
            obj = {"version": {"id": 100, "crate": deferred_a, "num": "0.1.0", "yanked": False, "checksum": deferred_baselines[deferred_a][1]}}
        elif deferred and p == f"/api/v1/crates/{deferred_a}/0.1.1":
            if deferred_a_visible():
                obj = {"version": {"id": 101, "crate": deferred_a, "num": "0.1.1", "yanked": False, "checksum": deferred_a_checksum, "features": {}}}
            else:
                status, obj = 404, {"errors": []}
        elif deferred and p in (f"/crates/{deferred_a}/{deferred_a}-0.1.0.crate", f"/crates/{deferred_a}/{deferred_a}-0.1.1.crate", f"/crates/{deferred_a}/0.1.0/download", f"/crates/{deferred_a}/0.1.1/download"):
            data = deferred_baselines[deferred_a][0] if p.endswith(("-0.1.0.crate", "/0.1.0/download")) else deferred_a_archive if deferred_a_visible() else None
            if data is None:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        elif deferred and p in (index_path(deferred_a), index_path(deferred_a.replace("-", "_"))):
            entries = [{"name": deferred_a, "vers": "0.1.0", "deps": [], "features": {}, "cksum": deferred_baselines[deferred_a][1], "yanked": False}]
            if deferred_a_visible():
                entries.append({"name": deferred_a, "vers": "0.1.1", "deps": [], "features": {}, "cksum": deferred_a_checksum, "yanked": False})
            data = "\n".join(json.dumps(entry) for entry in entries).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        elif p.startswith("/api/v1/crates/") and p.removeprefix("/api/v1/crates/") in dependency_snapshots:
            obj = dependency_snapshots[p.removeprefix("/api/v1/crates/")]
        elif p == f"/api/v1/crates/{package}":
            obj = {
                "crate": {"id": package, "name": package, "versions": [100]},
                "versions": [
                    {"id": 100, "crate": package, "num": baseline_version, "yanked": False}
                ],
            }
            alternate = active_alternate()
            if alternate and alternate["version"] != baseline_version:
                obj["crate"]["versions"].append(alternate["id"])
                obj["versions"].append({"id": alternate["id"], "crate": package, "num": alternate["version"], "yanked": False})
            if published:
                obj["crate"]["versions"].append(101)
                obj["versions"].append(
                    {"id": 101, "crate": package, "num": version, "yanked": False}
                )
        elif p == f"/api/v1/crates/{package}/owners":
            obj = {"users": [{"id": reviewer_id, "login": "owner", "kind": "user"}]}
        elif p == f"/api/v1/crates/{package}/{baseline_version}" or (active_alternate() and p == f"/api/v1/crates/{package}/{active_alternate()['version']}"):
            alternate = active_alternate()
            selected = alternate if alternate and p.endswith("/" + alternate["version"]) else {"id": 100, "version": baseline_version, "checksum": checksum}
            obj = {
                "version": {
                    "id": selected["id"],
                    "crate": package,
                    "num": selected["version"],
                    "yanked": False,
                    "checksum": selected["checksum"],
                }
            }
        elif p == f"/api/v1/crates/{package}/{version}":
            if published:
                obj = {
                    "version": {
                        "id": 101,
                        "crate": package,
                        "num": version,
                        "yanked": False,
                        "checksum": hashlib.sha256(published).hexdigest(),
                        "features": published_features,
                    }
                }
            else:
                status, obj = 404, {"errors": []}
        elif p == f"/crates/{package}/{package}-{baseline_version}.crate" or (active_alternate() and p == f"/crates/{package}/{package}-{active_alternate()['version']}.crate"):
            alternate = active_alternate()
            selected_archive = alternate["archive"] if alternate and p.endswith(f"-{alternate['version']}.crate") else archive
            self.send_response(200)
            self.send_header("Content-Length", str(len(selected_archive)))
            self.end_headers()
            self.wfile.write(selected_archive)
            return
        elif p == f"/crates/{package}/{package}-{version}.crate" and published:
            self.send_response(200)
            self.send_header("Content-Length", str(len(published)))
            self.end_headers()
            self.wfile.write(published)
            return
        elif p in (index_path(package), index_path(package.replace("-", "_"))) and published:
            obj = {
                "name": package,
                "vers": version,
                "deps": [],
                "features": published_features,
                "cksum": hashlib.sha256(published).hexdigest(),
                "yanked": False,
            }
        elif p in (index_path(package), index_path(package.replace("-", "_"))) and os.environ.get("VELNOR_FIXTURE_REAL_CARGO"):
            status, obj = 404, {"errors": []}
        else:
            status, obj = 500, {"unexpected": p}
        data = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_PUT(self):
        global upload_attempts
        upload_attempts += 1
        if not os.environ.get("VELNOR_FIXTURE_REAL_CARGO") or self.path != "/api/v1/crates/new":
            self.send_error(404)
            return
        if self.headers.get("Authorization") != "fixture-scoped":
            self.send_error(403)
            return
        length = int(self.headers.get("Content-Length", "0"))
        if not 8 <= length <= 16 * 1024 * 1024:
            self.send_error(413)
            return
        data = self.rfile.read(length)
        try:
            metadata_length = struct.unpack_from("<I", data)[0]
            metadata_end = 4 + metadata_length
            metadata = json.loads(data[4:metadata_end])
            archive_length = struct.unpack_from("<I", data, metadata_end)[0]
            archive = data[metadata_end + 4:]
            if len(archive) != archive_length or metadata["name"] != package or metadata["vers"] != version:
                raise ValueError("invalid upload")
        except (ValueError, KeyError, struct.error, json.JSONDecodeError):
            self.send_error(400)
            return
        self.record_upload(archive, 200)

    def record_upload(self, data, status):
        global mutations, published, published_features, uploads
        assert not published
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive_file:
            vcs = json.load(
                archive_file.extractfile(f"{package}-{version}/.cargo_vcs_info.json")
            )
            assert vcs["git"]["sha1"] == source_sha
            manifest = archive_file.extractfile(f"{package}-{version}/Cargo.toml").read()
            published_features = tomllib.loads(manifest.decode()).get("features", {})
            assert (
                archive_file.extractfile(f"{package}-{version}/src/lib.rs").read()
                == (source / ("b" if deferred else "") / "src/lib.rs").read_bytes()
            )
        if os.environ.get("VELNOR_FIXTURE_REAL_CARGO"):
            temporary = root / "fixture-upload.crate.tmp"
            with temporary.open("xb") as output:
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
            os.replace(temporary, root / "fixture-upload.crate")
            directory = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        published = data
        uploads += 1
        if os.environ.get("VELNOR_FIXTURE_FAILURE") in ("coordinate-crash", "coordinate-crash-no-credentials", "coordinate-real-cargo-crash", "coordinate-real-cargo-new-run"):
            pid_path = pathlib.Path(os.environ["VELNOR_FIXTURE_CRASH_PID_PATH"])
            os.kill(int(pid_path.read_text()), signal.SIGKILL)
        self.send_response(status)
        response = b'{"warnings":{"invalid_categories":[],"invalid_badges":[],"other":[]}}' if status == 200 else b""
        self.send_header("Content-Length", str(len(response)))
        self.end_headers()
        self.wfile.write(response)
        return

    def do_POST(self):
        global mutations, published, published_features, uploads, release, policy
        data = self.rfile.read(int(self.headers["Content-Length"]))
        if self.path == "/__fixture/advance-recovery":
            assert os.environ.get("VELNOR_FIXTURE_FAILURE") == "coordinate-real-cargo-new-run"
            assert published is not None and uploads == 1 and policy == prior_policy
            intent_path = root / ".github/velnor-release-intent.toml"
            with intent_path.open("a", encoding="utf-8") as intent_file:
                intent_file.write(f'\n[journal_predecessor]\nrun_id = 12\nrun_attempt = 1\nworkflow_sha = "{prior_policy}"\n')
            run("git", "add", ".github/velnor-release-intent.toml", cwd=root)
            run("git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-qm", "reviewed recovery", cwd=root)
            policy = run("git", "rev-parse", "HEAD", cwd=root)
            run("git", "fetch", "--quiet", "--no-recurse-submodules", "--no-tags",
                str(root), "refs/heads/*:refs/remotes/origin/*", cwd=source)
            run("git", "merge-base", "--is-ancestor", prior_policy, policy, cwd=source)
            assert run("git", "rev-parse", "HEAD", cwd=source) == source_sha
            payload = json.dumps({"workflow_sha": policy}).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        mutations += 1
        if self.path == "/__fixture/upload":
            return self.record_upload(data, 201)
        value = json.loads(data)
        status = 201
        if self.path == f"/repos/{repository}/git/blobs":
            assert value["encoding"] == "base64"
            raw = base64.b64decode(value["content"], validate=True)
            sha = git_hash("blob", raw)
            blobs[sha] = raw
            obj = {"sha": sha}
        elif self.path == f"/repos/{repository}/git/tags":
            tag, blob = value["tag"], value["object"]
            assert (
                tag.startswith("velnor-receipt/")
                and value["type"] == "blob"
                and blob in blobs
            )
            raw = f"object {blob}\ntype blob\ntag {tag}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1".encode()
            sha = git_hash("tag", raw)
            obj = {
                "sha": sha,
                "tag": tag,
                "message": "velnor-release-receipt-v1",
                "object": {"type": "blob", "sha": blob},
                "tagger": {
                    "name": "Velnor Release",
                    "email": "release@velnor.invalid",
                    "date": "1970-01-01T00:00:00Z",
                },
                "verification": {
                    "verified": False,
                    "reason": "unsigned",
                    "signature": None,
                    "payload": None,
                },
            }
            tags[sha] = obj
        elif self.path == f"/repos/{repository}/git/refs":
            name, sha = value["ref"], value["sha"]
            is_journal = name.startswith("refs/tags/velnor-receipt/")
            assert (
                is_journal and sha in tags and name == "refs/tags/" + tags[sha]["tag"]
            ) or (name == f"refs/tags/{package}-v{version}" and sha == source_sha) or (
                name in {f"refs/tags/{item}-v0.1.0" for item in multi_names} and sha == source_sha
            ) or (
                deferred and name == f"refs/tags/{deferred_a}-v0.1.1" and sha == source_sha
            )
            if name in refs:
                status, obj = 422, {}
            else:
                refs[name] = sha
                obj = {
                    "ref": name,
                    "object": {"type": "tag" if is_journal else "commit", "sha": sha},
                }
        elif self.path == f"/repos/{repository}/releases":
            tag_name = value["tag_name"]
            if deferred and tag_name in {f"{deferred_a}-v0.1.1", f"{deferred_b}-v0.1.1"}:
                assert value["target_commitish"] == source_sha and value["prerelease"] is False
                assert tag_name not in releases
                releases[tag_name] = {"id": len(releases) + 100, "tag_name": tag_name, "draft": False,
                                      "prerelease": False, "html_url": f"https://github.com/{repository}/releases/tag/{tag_name}"}
                data = json.dumps(releases[tag_name]).encode()
                self.send_response(201)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                return
            if multi_names and tag_name in {f"{item}-v0.1.0" for item in multi_names}:
                assert value["target_commitish"] == source_sha and value["prerelease"] is False
                assert tag_name not in releases
                releases[tag_name] = {"id": len(releases) + 100, "tag_name": tag_name, "draft": False,
                                      "prerelease": False, "html_url": f"https://github.com/{repository}/releases/tag/{tag_name}"}
                obj = releases[tag_name]
                data = json.dumps(obj).encode()
                self.send_response(201)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
                return
            assert (
                value["tag_name"] == f"{package}-v{version}"
                and value["target_commitish"] == source_sha
                and type(value["prerelease"]) is bool
                and value["prerelease"] == ("-" in version.split("+", 1)[0])
                and release is None
            )
            release = {
                "id": 99,
                "tag_name": value["tag_name"],
                "draft": False,
                "prerelease": value["prerelease"],
                "html_url": f"https://github.com/{repository}/releases/tag/{package}-v{version}",
            }
            obj = release
        else:
            status, obj = 405, {}
        data = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
origin = f"http://127.0.0.1:{server.server_port}"
info = {
    "origin": origin,
    "source_sha": source_sha,
    "workflow_sha": policy,
    "workspace": str(root),
    "package": package,
    "repository": repository,
    "version": version,
    "baseline_version": baseline_version,
    "baseline_checksum": checksum,
    "alternate_baseline_version": alternate_baseline["version"] if alternate_baseline else None,
    "alternate_baseline_checksum": alternate_baseline["checksum"] if alternate_baseline else None,
    "deferred_a": deferred_a if deferred else None,
    "deferred_b": deferred_b if deferred else None,
    "deferred_a_checksum": deferred_a_checksum if deferred else None,
    "deferred_a_baseline_checksum": deferred_baselines[deferred_a][1] if deferred else None,
}
print(json.dumps(info), flush=True)
server.serve_forever()

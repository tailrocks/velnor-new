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
source = root / "release-source"


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
supplied = os.environ.get("VELNOR_FIXTURE_SUPPLIED_INTENT")
supplied_intent = None
dependency_snapshots = {}
if supplied:
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
    write(source / "Cargo.toml", manifest.format("0.1.0"))
    write(source / "src/lib.rs", "pub fn example() -> u32 { 1 }\n")
    run("cargo", "generate-lockfile", "--offline", cwd=source)
    run("cargo", "package", "--offline", "--no-verify", "--allow-dirty", cwd=source)
    archive = (source / "target/package/example-0.1.0.crate").read_bytes()
    checksum = hashlib.sha256(archive).hexdigest()
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
rust_version = "1.97.1"
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
if supplied_intent is not None:
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
    f"name: release\nconcurrency:\n  group: release-crates-io-{repository}-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n  cancel-in-progress: false\njobs: {{}}\n",
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

mutations = 0
blobs = {}
tags = {}
refs = {}
published = None
published_features = {}
release = None
uploads = 0


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
        sha = policy if policy in p else source_sha
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
                "published_sha256": hashlib.sha256(published).hexdigest()
                if published
                else None,
                "release": release,
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
        elif p.startswith(f"/repos/{repository}/actions/runs/12/approvals"):
            obj = [
                {
                    "state": "approved",
                    "user": {"type": "User", "id": reviewer_id},
                    "environments": [{"id": 9, "name": "release"}],
                    "comment": f"I reviewed the complete release policy at {policy} and source {source_sha}, and authorize this source for run 12.",
                }
            ]
            if os.environ.get("VELNOR_FIXTURE_FAILURE") == "approval":
                obj[0]["comment"] = "Unrelated approval"
        elif p.startswith(f"/repos/{repository}/actions/runs/12"):
            obj = {
                "id": 12,
                "run_attempt": 1,
                "head_sha": policy,
                "head_branch": "main",
                "event": "workflow_dispatch",
                "path": ".github/workflows/release.yml",
                "repository": {"id": 22, "full_name": repository, "owner": {"id": 77, "login": repository.split("/", 1)[0]}},
                "head_repository": {"id": 22, "full_name": repository, "owner": {"id": 77, "login": repository.split("/", 1)[0]}},
            }
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
                        "check_suite_id": 123,
                        "head_sha": sha,
                        "path": required_workflow,
                        "status": "completed",
                        "conclusion": "success",
                    }
                ],
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
            if release is None:
                status, obj = 404, {"message": "Not Found"}
            else:
                obj = release
        elif p == "/users/owner":
            obj = {"id": reviewer_id, "login": "owner", "type": "User"}
        elif p.startswith("/api/v1/crates/") and p.removeprefix("/api/v1/crates/") in dependency_snapshots:
            obj = dependency_snapshots[p.removeprefix("/api/v1/crates/")]
        elif p == f"/api/v1/crates/{package}":
            obj = {
                "crate": {"id": package, "name": package, "versions": [100]},
                "versions": [
                    {"id": 100, "crate": package, "num": baseline_version, "yanked": False}
                ],
            }
            if published:
                obj["crate"]["versions"].append(101)
                obj["versions"].append(
                    {"id": 101, "crate": package, "num": version, "yanked": False}
                )
        elif p == f"/api/v1/crates/{package}/owners":
            obj = {"users": [{"id": reviewer_id, "login": "owner", "kind": "user"}]}
        elif p == f"/api/v1/crates/{package}/{baseline_version}":
            obj = {
                "version": {
                    "id": 100,
                    "crate": package,
                    "num": baseline_version,
                    "yanked": False,
                    "checksum": checksum,
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
        elif p == f"/crates/{package}/{package}-{baseline_version}.crate":
            self.send_response(200)
            self.send_header("Content-Length", str(len(archive)))
            self.end_headers()
            self.wfile.write(archive)
            return
        elif p == f"/crates/{package}/{package}-{version}.crate" and published:
            self.send_response(200)
            self.send_header("Content-Length", str(len(published)))
            self.end_headers()
            self.wfile.write(published)
            return
        elif p == index_path(package) and published:
            obj = {
                "name": package,
                "vers": version,
                "cksum": hashlib.sha256(published).hexdigest(),
                "yanked": False,
            }
        else:
            status, obj = 500, {"unexpected": p}
        data = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        global mutations, published, published_features, uploads, release
        mutations += 1
        data = self.rfile.read(int(self.headers["Content-Length"]))
        if self.path == "/__fixture/upload":
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
                    == (source / "src/lib.rs").read_bytes()
                )
            published = data
            uploads += 1
            if os.environ.get("VELNOR_FIXTURE_FAILURE") in ("coordinate-crash", "coordinate-crash-no-credentials"):
                pid_path = pathlib.Path(os.environ["VELNOR_FIXTURE_CRASH_PID_PATH"])
                os.kill(int(pid_path.read_text()), signal.SIGKILL)
            self.send_response(201)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
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
            ) or (name == f"refs/tags/{package}-v{version}" and sha == source_sha)
            if name in refs:
                status, obj = 422, {}
            else:
                refs[name] = sha
                obj = {
                    "ref": name,
                    "object": {"type": "tag" if is_journal else "commit", "sha": sha},
                }
        elif self.path == f"/repos/{repository}/releases":
            assert (
                value["tag_name"] == f"{package}-v{version}"
                and value["target_commitish"] == source_sha
                and release is None
            )
            release = {
                "id": 99,
                "tag_name": value["tag_name"],
                "draft": False,
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
}
print(json.dumps(info), flush=True)
server.serve_forever()

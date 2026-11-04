# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import http.server
import json
import os
import pathlib
import subprocess
import sys

root = pathlib.Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=True)
source = root / "release-source"
source.mkdir()


def run(*args, cwd):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(data)


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
import shutil

shutil.rmtree(source / "target")
intent = f'''schema = 1
repository = "owner/project"
default_branch = "main"
environment = "release"
required_workflow_path = ".github/workflows/ci.yml"
trusted_reviewer_id = 139017
rust_version = "1.97.1"
generator_version = "0.1.5"
generator_source_sha = "f1041f322c54cd7edfdef06afd307799eeaa801a"
generator_manifest_sha256 = "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52"
release_plz_version = "0.4.3"
runner = "ubuntu-24.04"
authentication = "trusted-publishing"
manifest_path = "Cargo.toml"
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
write(root / ".github/velnor-release-intent.toml", intent)
run("git", "init", "-q", cwd=root)
run("git", "add", ".github/velnor-release-intent.toml", cwd=root)
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


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        sys.stderr.write("fixture: " + fmt % args + "\n")

    def do_GET(self):
        p = self.path
        sha = policy if policy in p else source_sha
        status = 200
        obj = None
        if p == "/__fixture/state":
            obj = {"mutations": mutations}
        elif p == "/repos/owner/project":
            obj = {"id": 22, "full_name": "owner/project", "default_branch": "main"}
        elif p == "/repos/owner/project/git/ref/heads/main":
            obj = {
                "ref": "refs/heads/main",
                "object": {"type": "commit", "sha": policy},
            }
        elif p.startswith("/repos/owner/project/actions/runs/12/approvals"):
            obj = [
                {
                    "state": "approved",
                    "user": {"type": "User", "id": 139017},
                    "environments": [{"id": 9, "name": "release"}],
                    "comment": f"I reviewed the complete release policy at {policy} and source {source_sha}, and authorize this source for run 12.",
                }
            ]
            if os.environ.get("VELNOR_FIXTURE_FAILURE") == "approval":
                obj[0]["comment"] = "Unrelated approval"
        elif p.startswith("/repos/owner/project/actions/runs/12"):
            obj = {
                "id": 12,
                "run_attempt": 1,
                "head_sha": policy,
                "head_branch": "main",
                "event": "workflow_dispatch",
                "path": ".github/workflows/release.yml",
                "repository": {"id": 22, "full_name": "owner/project"},
                "head_repository": {"id": 22, "full_name": "owner/project"},
            }
        elif "/compare/" in p:
            obj = {"status": "ahead"}
        elif p.startswith(
            "/repos/owner/project/environments/release/deployment-branch-policies?"
        ):
            obj = {
                "total_count": 1,
                "branch_policies": [{"type": "branch", "name": "main"}],
            }
        elif p == "/repos/owner/project/environments/release":
            obj = {
                "id": 9,
                "name": "release",
                "can_admins_bypass": False,
                "deployment_branch_policy": {"custom_branch_policies": True},
                "protection_rules": [
                    {
                        "type": "required_reviewers",
                        "reviewers": [{"type": "User", "reviewer": {"id": 139017}}],
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
                        "path": ".github/workflows/ci.yml",
                        "status": "completed",
                        "conclusion": "success",
                    }
                ],
            }
        elif p.startswith("/repos/owner/project/rulesets?"):
            obj = [{"id": 1}]
        elif p == "/repos/owner/project/rulesets/1":
            obj = {
                "id": 1,
                "target": "tag",
                "enforcement": "active",
                "bypass_actors": [],
                "conditions": {"ref_name": {"include": ["~ALL"], "exclude": []}},
                "rules": [{"type": "update"}, {"type": "deletion"}],
            }
        elif p.startswith("/repos/owner/project/git/ref/tags/"):
            status, obj = 404, {"message": "Not Found"}
        elif p == "/users/owner":
            obj = {"id": 139017, "login": "owner", "type": "User"}
        elif p == "/api/v1/crates/example":
            obj = {
                "crate": {"id": "example", "name": "example", "versions": [100]},
                "versions": [
                    {"id": 100, "crate": "example", "num": "0.1.0", "yanked": False}
                ],
            }
        elif p == "/api/v1/crates/example/owners":
            obj = {"users": [{"id": 139017, "login": "owner", "kind": "user"}]}
        elif p == "/api/v1/crates/example/0.1.0":
            obj = {
                "version": {
                    "id": 100,
                    "crate": "example",
                    "num": "0.1.0",
                    "yanked": False,
                    "checksum": checksum,
                }
            }
        elif p == "/api/v1/crates/example/0.1.1":
            status, obj = 404, {"errors": []}
        elif p == "/crates/example/example-0.1.0.crate":
            self.send_response(200)
            self.send_header("Content-Length", str(len(archive)))
            self.end_headers()
            self.wfile.write(archive)
            return
        else:
            status, obj = 500, {"unexpected": p}
        data = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        global mutations
        mutations += 1
        self.send_error(405)


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
origin = f"http://127.0.0.1:{server.server_port}"
info = {
    "origin": origin,
    "source_sha": source_sha,
    "workflow_sha": policy,
    "workspace": str(root),
    "package": "example",
    "version": "0.1.1",
    "baseline_checksum": checksum,
}
print(json.dumps(info), flush=True)
server.serve_forever()

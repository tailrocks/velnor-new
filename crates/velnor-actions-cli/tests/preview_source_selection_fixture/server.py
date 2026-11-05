# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import http.server
import json
import pathlib
import ssl
import sys
import urllib.parse

root = pathlib.Path(sys.argv[1])
tls = pathlib.Path(sys.argv[2])
scenario = sys.argv[3]
source, authority, protected, wrong = (letter * 40 for letter in "abcd")
tree, manifest, main = "e" * 40, "f" * 40, "0" * 40
base = "/repos/test-owner/test-tap"
repo = {"id": 819273, "full_name": "test-owner/test-tap"}


def run(run_id, workflow_id, workflow, sha, event, status, conclusion=None):
    return {"id": run_id, "run_attempt": 1, "run_number": run_id, "workflow_id": workflow_id,
            "name": workflow, "path": ".github/workflows/" + ("ci.yml" if workflow_id == 10 else "binary-preview.yml"),
            "head_sha": sha, "head_branch": "main", "event": event, "status": status,
            "conclusion": conclusion, "repository": repo, "head_repository": repo}


ci_run = run(100, 10, "CI", source, "push", "completed", "success")
preview_run = run(200, 20, "Rust binary preview", authority, "workflow_run", "in_progress")


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        parsed = urllib.parse.urlsplit(self.path)
        accept = self.headers.get("Accept", "")
        with (root / "requests.tsv").open("a") as log:
            log.write(accept + "\t" + self.path.lstrip("/") + "\n")
        if self.headers.get("Authorization") not in ("token fixture_token", "token fixture-status-read-token"):
            self.send_error(401)
            return
        path = parsed.path
        if path == base:
            body = {**repo, "id": 819274 if scenario == "wrong-repository" else 819273, "default_branch": "main"}
        elif path == base + "/rules/branches/main":
            body = [{"type": "pull_request", "parameters": {"required_approving_review_count": 1, "required_review_thread_resolution": True}}, {"type": "required_status_checks", "parameters": {"required_status_checks": [{"context": "Required"}]}}, {"type": "non_fast_forward"}, {"type": "deletion"}]
        elif path == base + "/branches/main/protection":
            self.respond(404, {"status": "404", "message": "Not Found"})
            return
        elif path == base + "/environments/preview-publish":
            body = {"name": "preview-publish", "deployment_branch_policy": {"protected_branches": False, "custom_branch_policies": True}, "protection_rules": [{"type": "branch_policy"}]}
        elif path == base + "/environments/preview-publish/deployment-branch-policies":
            body = {"total_count": 1, "branch_policies": [{"name": "main"}]}
        elif path == base + "/branches/main":
            body = {"name": "main", "protected": True, "commit": {"sha": protected}}
        elif path.startswith(base + "/commits/"):
            sha = path.rsplit("/", 1)[-1]
            reported = wrong if scenario == "wrong-sha" and sha == source else sha
            if accept == "application/vnd.github.sha":
                self.respond(200, reported, "text/plain")
                return
            body = {"sha": reported, "padding": "x" * 1048700} if scenario == "large-json" else {"sha": reported}
        elif path.startswith(base + "/compare/"):
            pair = path.rsplit("/", 1)[-1]
            first = source if pair == source + "..." + authority else authority if pair == authority + "..." + protected else None
            if first is None:
                self.send_error(404); return
            body = {"base_commit": {"sha": first}, "merge_base_commit": {"sha": first}, "status": "ahead", "ahead_by": 1, "total_commits": 1}
        elif path.startswith(base + "/git/commits/"):
            sha = path.rsplit("/", 1)[-1]
            if sha == source:
                body = {"sha": source, "tree": {"sha": tree}, "parents": []}
            elif sha == authority:
                body = {"sha": authority, "parents": [{"sha": source}]}
            elif sha == protected:
                body = {"sha": protected, "parents": [{"sha": authority}]}
            else:
                self.send_error(404); return
        elif path.startswith(base + "/git/trees/"):
            body = {"sha": tree, "truncated": False, "tree": [{"path": "Cargo.toml", "mode": "100644", "type": "blob", "sha": manifest}, {"path": "src/main.rs", "mode": "100644", "type": "blob", "sha": main}]}
        elif path in (base + "/actions/workflows/ci.yml", base + "/actions/workflows/binary-preview.yml"):
            is_ci = path.endswith("ci.yml")
            body = {"id": 10 if is_ci else 20, "name": "CI" if is_ci else "Rust binary preview", "path": ".github/workflows/" + ("ci.yml" if is_ci else "binary-preview.yml"), "state": "active"}
        elif path in (base + "/actions/runs/100/attempts/1", base + "/actions/runs/100"):
            body = ci_run
        elif path == base + "/actions/runs/100/attempts/1/jobs":
            body = {"total_count": 1, "jobs": [{"id": 101, "run_id": 100, "name": "Required", "head_sha": source, "status": "completed", "conclusion": "success"}]}
        elif path == base + "/actions/workflows/10/runs":
            body = {"workflow_runs": [ci_run]}
        elif path in (base + "/actions/runs/200/attempts/1", base + "/actions/runs/200"):
            body = preview_run
        elif path == base + "/actions/runs/200/attempts/1/jobs":
            body = {"total_count": 1, "jobs": [{"id": 201, "run_id": 200, "name": "Select authenticated main source for preview", "head_sha": authority, "status": "in_progress", "started_at": "2026-10-27T00:00:00Z"}]}
        else:
            self.send_error(404)
            return
        self.respond(200, body)

    def respond(self, status, body, content_type="application/json"):
        data = body.encode() if isinstance(body, str) else json.dumps(body, separators=(",", ":")).encode()
        self.send_response(status); self.send_header("Content-Type", content_type); self.send_header("Content-Length", str(len(data))); self.end_headers()
        try:
            self.wfile.write(data)
        except (BrokenPipeError, ConnectionResetError):
            pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 443), Handler)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(tls / "cert.pem", tls / "key.pem")
server.socket = context.wrap_socket(server.socket, server_side=True)
(root / "ready").write_text("ready")
server.serve_forever()

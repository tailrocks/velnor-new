#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import argparse
import datetime
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tomllib

USER_AGENT = "velnor-freshness-capture"
RUST_PREFIX_BYTES = 128 * 1024
RUST_SOURCE = "https://static.rust-lang.org/dist/channel-rust-stable.toml"


def stamp():
    now = datetime.datetime.now(datetime.timezone.utc)
    return now.strftime("%Y-%m-%dT%H:%M:%S.") + f"{now.microsecond // 1000:03d}Z"


def norm(version):
    version = version.strip()
    version = version.removeprefix("bun-")
    version = version.removeprefix("v")
    return version.split("+")[0]


def run(argv):
    result = subprocess.run(argv, capture_output=True, check=False)
    if result.returncode != 0:
        raise SystemExit(f"{' '.join(argv)}: exit {result.returncode}: {result.stderr.decode(errors='replace')[:300]}")
    return result.stdout


def fetch(url):
    if url.startswith("https://api.github.com/"):
        return run(["gh", "api", url]), None
    if url == RUST_SOURCE:
        headers = run(["curl", "-sS", "--fail", "-A", USER_AGENT, "-r", f"0-{RUST_PREFIX_BYTES - 1}", "-D", "-", "-o", "/dev/null", url]).decode()
        body = run(["curl", "-sS", "--fail", "-A", USER_AGENT, "-r", f"0-{RUST_PREFIX_BYTES - 1}", url])
        status = int(headers.split()[1])
        content_range = re.search(r"(?im)^content-range:\s*(.+?)\s*$", headers)
        return body, {"status": status, "content_range": content_range.group(1) if content_range else None, "bytes": len(body)}
    return run(["curl", "-sS", "--fail", "--compressed", "-A", USER_AGENT, url]), None


def observe(source, body):
    if source == RUST_SOURCE:
        table = []
        inside = False
        for line in body.decode().splitlines():
            text = line.strip()
            if text.startswith("[") and text.endswith("]"):
                if inside:
                    break
                inside = text == "[pkg.rust]"
                if inside:
                    table.append(text)
            elif inside:
                table.append(line)
        version = tomllib.loads("\n".join(table))["pkg"]["rust"]["version"].split()[0]
        return {"version": version}
    payload = json.loads(body)
    if "crates.io/api/v1/crates/" in source:
        return {"version": payload["crate"]["max_version"]}
    release = payload
    if isinstance(payload, list):
        release = next(item for item in payload if not item.get("draft") and not item.get("prerelease"))
    return {
        "version": release["tag_name"],
        "release_id": release["id"],
        "published_at": release["published_at"],
        "prerelease": release["prerelease"],
        "draft": release["draft"],
    }


def repo_of(source):
    match = re.match(r"https://api\.github\.com/repos/([^/]+/[^/]+)/", source)
    return match.group(1) if match else None


def slug(subject):
    return subject.replace("/", "_")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


class Capture:
    def __init__(self, out, name):
        self.out = out
        self.name = name
        self.receipts = []
        self.comparison = []
        self.observed_at = {"tools": {}, "actions": {}}
        self.latest_sha = {}
        out.mkdir(parents=True, exist_ok=True)

    def receipt(self, subject, source, requested, observed_at, body_file, body, observed):
        (self.out / body_file).write_bytes(body)
        self.receipts.append({
            "subject": subject,
            "source": source,
            "requested_at": requested,
            "observed_at": observed_at,
            "body_file": body_file,
            "bytes": len(body),
            "sha256": sha256(body),
            "observed": observed,
        })

    def tag(self, subject, repo, version, role):
        prefix = f"{role}-{slug(subject)}"
        ref_url = f"https://api.github.com/repos/{repo}/git/ref/tags/{version}"
        requested = stamp()
        body, _ = fetch(ref_url)
        ref = json.loads(body)
        self.receipt(f"{subject} {role} tag-ref", ref_url, requested, stamp(), f"{prefix}-ref.body", body, ref)
        obj = ref["object"]
        while obj["type"] == "tag":
            tag_url = f"https://api.github.com/repos/{repo}/git/tags/{obj['sha']}"
            requested = stamp()
            body, _ = fetch(tag_url)
            tag = json.loads(body)
            self.receipt(f"{subject} {role} tag-tagobj", tag_url, requested, stamp(), f"{prefix}-tagobj.body", body, {"tag": tag["tag"], "sha": tag["sha"], "object": tag["object"]})
            obj = tag["object"]
        if obj["type"] != "commit":
            raise SystemExit(f"{subject} {role}: tag {version} resolves to {obj['type']}")
        self.receipts.append({
            "subject": f"{subject} {role} tag",
            "source": ref_url,
            "observed_at": stamp(),
            "observed": {"version": version, "sha": obj["sha"]},
        })
        return obj["sha"]

    def subject(self, name, kind, source, pinned, pinned_sha):
        requested = stamp()
        body, extra = fetch(source)
        observed_at = stamp()
        observed = observe(source, body)
        self.receipt(name, source, requested, observed_at, f"{slug(name)}.body", body, observed)
        if extra is not None:
            (self.out / "rust.headers.json").write_text(json.dumps(extra, indent=2))
        self.observed_at[kind][name] = observed_at
        self.comparison.append({
            "subject": name,
            "pinned": pinned,
            "observed": observed["version"],
            "match": norm(pinned) == norm(observed["version"]),
        })
        repo = repo_of(source)
        if repo and (kind == "actions" or name == "mr-boxington"):
            pinned_commit = self.tag(name, repo, pinned if kind == "actions" else f"v{pinned}", "pinned")
            if pinned_sha is not None and pinned_commit != pinned_sha:
                raise SystemExit(f"{name}: pinned tag resolves to {pinned_commit}, inventory pins {pinned_sha}")
            latest_commit = self.tag(name, repo, observed["version"], "latest")
            self.latest_sha[name] = latest_commit

    def finish(self):
        (self.out / "comparison.json").write_text(json.dumps(self.comparison, indent=2))
        (self.out / "observed-at.json").write_text(json.dumps(self.observed_at, indent=2))
        (self.out / "receipts.json").write_text(json.dumps({"schema": 1, "capture": self.name, "source_responses": self.receipts}, indent=2))


def run_capture(root, evidence, name):
    inventory = json.loads((root / ".velnor/freshness-inventory.json").read_text())
    capture = Capture(evidence / name, name)
    for tool in inventory["tools"]:
        capture.subject(tool["name"], "tools", tool["source"], tool["pinned"], None)
    for action in inventory["actions"]:
        capture.subject(action["key"], "actions", action["source"], action["pinned_version"], action["pinned_sha"])
    capture.finish()
    behind = [row["subject"] for row in capture.comparison if not row["match"]]
    print(f"{name}: {len(capture.receipts)} receipts, behind: {behind or 'none'}")


def agree(evidence):
    captures = []
    for name in ("capture-1", "capture-2"):
        comparison = (evidence / name / "comparison.json").read_bytes()
        receipts = json.loads((evidence / name / "receipts.json").read_text())
        captures.append({
            "name": name,
            "comparison_sha256": sha256(comparison),
            "receipts_sha256": sha256((evidence / name / "receipts.json").read_bytes()),
            "rows": json.loads(comparison),
            "observed": {r["subject"]: r["observed"] for r in receipts["source_responses"] if "requested_at" in r or "observed" in r},
        })
    first, second = captures
    same = first["comparison_sha256"] == second["comparison_sha256"] and first["observed"] == second["observed"]
    result = {"schema": 1, "captures": [{k: v for k, v in c.items() if k != "observed"} for c in captures], "agree": same}
    (evidence / "capture-agreement.json").write_text(json.dumps(result, indent=2))
    print(f"agree: {same}")
    if not same:
        raise SystemExit(1)


def apply(root, evidence, name, bumped):
    path = root / ".velnor/freshness-inventory.json"
    inventory = json.loads(path.read_text())
    receipts = json.loads((evidence / name / "receipts.json").read_text())["source_responses"]
    observed = {r["subject"]: r for r in receipts if "body_file" in r and " " not in r["subject"]}
    latest_sha = {r["subject"].removesuffix(" latest tag"): r["observed"]["sha"] for r in receipts if r["subject"].endswith(" latest tag")}
    for tool in inventory["tools"]:
        row = observed[tool["name"]]
        tool["latest"] = row["observed"]["version"]
        tool["checked_at"] = row["observed_at"]
        if tool["name"] in bumped:
            tool["qualified"] = tool["pinned"]
        if "latest_sha" in tool:
            tool["latest_sha"] = latest_sha[tool["name"]]
        tool["status"] = "current" if norm(tool["pinned"]) == norm(tool["latest"]) else "behind"
    for action in inventory["actions"]:
        row = observed[action["key"]]
        action["latest"] = row["observed"]["version"]
        action["checked_at"] = row["observed_at"]
        if "latest_sha" in action:
            action["latest_sha"] = latest_sha[action["key"]]
        action["status"] = "current" if norm(action["pinned_version"]) == norm(action["latest"]) else "behind"
    inventory["checked_at"] = min(x["checked_at"] for x in inventory["tools"] + inventory["actions"])
    path.write_text(json.dumps(inventory, indent=2) + "\n")
    behind = [x.get("name", x.get("key")) for x in inventory["tools"] + inventory["actions"] if x["status"] != "current"]
    print(f"applied {name}; not current: {behind or 'none'}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["capture", "agree", "apply"])
    parser.add_argument("--root", type=pathlib.Path, required=True)
    parser.add_argument("--evidence", type=pathlib.Path, required=True)
    parser.add_argument("--name", default="capture-1")
    parser.add_argument("--bumped", default="")
    args = parser.parse_args()
    if args.command == "capture":
        run_capture(args.root, args.evidence, args.name)
    elif args.command == "agree":
        agree(args.evidence)
    else:
        apply(args.root, args.evidence, args.name, set(filter(None, args.bumped.split(","))))


if __name__ == "__main__":
    sys.exit(main())

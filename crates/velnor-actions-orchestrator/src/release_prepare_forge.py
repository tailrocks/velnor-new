"""Credentialed release proposal consumer; mutations use fixed GitHub endpoints."""
import base64
import sys
import subprocess
import zipfile
from urllib.error import HTTPError
from urllib.request import HTTPRedirectHandler, Request, build_opener


PREPARATION_PROGRESS = None


class PrepareNoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, file, code, message, headers, url):
        raise ReconcileError("prepare_api_redirect")


def prepare_api(endpoint, payload=None, method="POST"):
    require(re.fullmatch(
        r"repos/[A-Za-z0-9._-]+/[A-Za-z0-9._-]+/(?:git/(?:blobs|trees|commits|refs)|pulls)",
        endpoint), "prepare_endpoint")
    token = os.environ.get("GH_TOKEN", "")
    require(token and "\n" not in token and "\r" not in token, "prepare_token")
    request = Request("https://api.github.com/" + endpoint,
                      data=None if payload is None else json.dumps(payload).encode(),
                      headers={"Authorization": "Bearer " + token,
                               "Accept": "application/vnd.github+json",
                               "X-GitHub-Api-Version": "2022-11-28",
                               "User-Agent": "velnor-fixed-release-preparation"},
                      method="GET" if payload is None else method)
    try:
        with build_opener(PrepareNoRedirect()).open(request, timeout=40) as response:
            require(response.status in (200, 201), "prepare_api_status")
            raw = response.read(4 * 1024 * 1024 + 1)
    except HTTPError as error:
        raise ReconcileError("prepare_api_status_" + str(error.code)) from None
    require(len(raw) <= 4 * 1024 * 1024, "prepare_api_size")
    value = decode_json(raw)
    require(isinstance(value, dict), "prepare_api_object")
    return value


def prepare_identity(approved):
    repository = approved["repository"]
    base = os.environ["RELEASE_DEFAULT_BRANCH"]
    require(re.fullmatch(r"[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*", base), "prepare_base")
    metadata = forge_api(f"repos/{repository}")
    require(isinstance(metadata, dict) and metadata.get("full_name") == repository and
            metadata.get("default_branch") == base, "prepare_repository")
    current = forge_api(f"repos/{repository}/git/ref/heads/{quote(base, safe='')}")
    require(isinstance(current, dict) and current.get("ref") == "refs/heads/" + base and
            current.get("object", {}).get("type") == "commit" and
            current["object"].get("sha") == approved["source_sha"], "prepare_base_changed")
    return base, 41898282


def prepare_existing_pr(approved, branch, base, actor_id, title, body, head, allow_missing=False):
    repository = approved["repository"]
    owner = repository.split("/")[0]
    prs = forge_api(f"repos/{repository}/pulls?state=all&head={quote(owner + ':' + branch, safe='')}&per_page=100")
    require(isinstance(prs, list) and len(prs) < 100, "prepare_pr_list")
    if not prs and allow_missing:
        return None
    require(len(prs) == 1, "prepare_existing_branch_unowned")
    pr = prs[0]
    require(pr.get("state") == "open" and pr.get("user", {}).get("id") == actor_id and
            pr.get("user", {}).get("login") == "github-actions[bot]" and
            pr.get("user", {}).get("type") == "Bot" and
            pr.get("title") == title and pr.get("body") == body and
            pr.get("base", {}).get("ref") == base and
            pr.get("base", {}).get("repo", {}).get("full_name") == repository and
            pr.get("head", {}).get("ref") == branch and pr.get("head", {}).get("sha") == head and
            pr.get("head", {}).get("repo", {}).get("full_name") == repository,
            "prepare_existing_pr_identity")
    return pr


def prepare_expected_tree(entries, changes):
    expected = dict(entries)
    expected.update({path: {key: value[key] for key in ("sha", "mode", "type")}
                     for path, value in changes.items()})
    return expected


def prepare_existing_head(approved, sha, expected):
    commit = forge_api(f"repos/{approved['repository']}/git/commits/{sha}")
    require(isinstance(commit, dict) and commit.get("sha") == sha and
            [item.get("sha") for item in commit.get("parents", [])] == [approved["source_sha"]],
            "prepare_existing_commit")
    require(prepare_tree(approved, commit.get("tree", {}).get("sha")) == expected,
            "prepare_existing_tree")


def prepare_create_head(approved, tree_sha, changes, expected, title):
    repository = approved["repository"]
    tree_entries = []
    for path, value in sorted(changes.items()):
        blob = prepare_api(f"repos/{repository}/git/blobs",
                           {"content": base64.b64encode(value["raw"]).decode(), "encoding": "base64"})
        require(blob.get("sha") == value["sha"], "prepare_created_blob")
        tree_entries.append({"path": path, "sha": value["sha"], "mode": "100644", "type": "blob"})
    tree = prepare_api(f"repos/{repository}/git/trees", {"base_tree": tree_sha, "tree": tree_entries})
    require(isinstance(tree.get("sha"), str) and re.fullmatch(r"[0-9a-f]{40}", tree["sha"]), "prepare_created_tree")
    require(prepare_tree(approved, tree["sha"]) == expected, "prepare_created_tree_contents")
    commit = prepare_api(f"repos/{repository}/git/commits",
                         {"message": title, "tree": tree["sha"], "parents": [approved["source_sha"]]})
    sha = commit.get("sha")
    require(isinstance(sha, str) and re.fullmatch(r"[0-9a-f]{40}", sha), "prepare_created_commit")
    prepare_existing_head(approved, sha, expected)
    return sha


def prepare_pull_request(approved=None):
    global PREPARATION_PROGRESS
    PREPARATION_PROGRESS = None
    approved = approved or policy()
    base, actor_id = prepare_identity(approved)
    tree_sha, entries = prepare_source_tree(approved)
    proposal, artifact, changes, multiple_public = prepare_proposal(approved, base, tree_sha, entries)
    if not changes:
        return {"status": "noop", "source_sha": approved["source_sha"], "artifact": artifact}
    title, body = prepare_pr_body(approved, proposal, multiple_public)
    branch = "velnor-release/" + approved["intent_id"]
    repository = approved["repository"]
    existing = forge_api(f"repos/{repository}/git/ref/heads/{quote(branch, safe='')}")
    expected = prepare_expected_tree(entries, changes)
    if existing is not None:
        require(existing.get("ref") == "refs/heads/" + branch and
                existing.get("object", {}).get("type") == "commit", "prepare_existing_ref")
        head = existing["object"].get("sha")
        require(isinstance(head, str) and re.fullmatch(r"[0-9a-f]{40}", head), "prepare_existing_sha")
        pr = prepare_existing_pr(approved, branch, base, actor_id, title, body, head, allow_missing=True)
        prepare_existing_head(approved, head, expected)
    else:
        head = prepare_create_head(approved, tree_sha, changes, expected, title)
        prepare_identity(approved)
        ref = prepare_api(f"repos/{repository}/git/refs", {"ref": "refs/heads/" + branch, "sha": head})
        require(ref.get("ref") == "refs/heads/" + branch and ref.get("object", {}).get("sha") == head,
                "prepare_created_ref")
        pr = None
    if pr is None:
        prepare_identity(approved)
        PREPARATION_PROGRESS = {"status": "branch-created", "phase": "create-pull-request",
                                "source_sha": approved["source_sha"], "branch": branch,
                                "head_sha": head, "artifact": artifact}
        save_receipt(PREPARATION_PROGRESS, "release-preparation/evidence.json")
        prepare_api(f"repos/{repository}/pulls", {"title": title, "body": body, "head": branch, "base": base})
        pr = prepare_existing_pr(approved, branch, base, actor_id, title, body, head)
    prepare_identity(approved)
    final_ref = forge_api(f"repos/{repository}/git/ref/heads/{quote(branch, safe='')}")
    require(isinstance(final_ref, dict) and final_ref.get("ref") == "refs/heads/" + branch and
            final_ref.get("object", {}).get("type") == "commit" and
            final_ref["object"].get("sha") == head, "prepare_final_ref_changed")
    pr = prepare_existing_pr(approved, branch, base, actor_id, title, body, head)
    number = pr.get("number")
    require(type(number) is int and number > 0 and
            pr.get("html_url") == f"https://github.com/{repository}/pull/{number}", "prepare_pr_url")
    return {"status": "prepared", "source_sha": approved["source_sha"], "artifact": artifact,
            "branch": branch, "head_sha": head, "pull_request": number, "url": pr["html_url"]}


def preparation_forge_main():
    try:
        receipt = prepare_pull_request()
        save_receipt(receipt, "release-preparation/evidence.json")
    except (ReconcileError, KeyError, TypeError, OSError, UnicodeError,
            json.JSONDecodeError, tomllib.TOMLDecodeError, ValueError,
            zipfile.BadZipFile, subprocess.SubprocessError) as error:
        reason = str(error) if isinstance(error, ReconcileError) else type(error).__name__
        save_receipt({**(PREPARATION_PROGRESS or {}), "status": "failed", "reason": reason},
                     "release-preparation/evidence.json")
        print("release preparation failed: " + reason, file=sys.stderr)
        raise SystemExit(1) from None
    return 0


if __name__ == "__main__":
    sys.exit(preparation_forge_main())

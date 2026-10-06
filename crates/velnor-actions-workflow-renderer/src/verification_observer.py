"""Fixed nightly issue observer; never consumes repository programs or logs."""
import json
import os
import re
import subprocess


TITLE = "Nightly CI red"


def gh_api(method, path, payload=None):
    argv = ["gh", "api", "--method", method, path]
    if payload is not None:
        argv.extend(["--input", "-"])
    result = subprocess.run(
        argv, input=None if payload is None else json.dumps(payload),
        text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
        timeout=60,
    )
    if result.returncode:
        raise RuntimeError("nightly issue API failed")
    return json.loads(result.stdout)


def validated_context(env):
    repository = env.get("APPROVED_REPOSITORY", "")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("invalid approved repository")
    if repository != env.get("GITHUB_REPOSITORY"):
        raise ValueError("repository mismatch")
    branch = env.get("DEFAULT_BRANCH", "")
    if not branch or env.get("REF") != "refs/heads/" + branch:
        raise ValueError("default branch required")
    if env.get("REF_PROTECTED") != "true":
        raise ValueError("protected branch required")
    if env.get("EVENT_NAME") not in {"schedule", "workflow_dispatch"}:
        raise ValueError("cadence event required")
    sha = env.get("SOURCE_SHA", "")
    if not re.fullmatch(r"[0-9a-fA-F]{40}", sha):
        raise ValueError("invalid source SHA")
    run_id, attempt = env.get("RUN_ID", ""), env.get("RUN_ATTEMPT", "")
    if not all(re.fullmatch(r"[1-9][0-9]*", value) for value in (run_id, attempt)):
        raise ValueError("invalid run identity")
    result = env.get("REQUIRED_RESULT", "")
    if result not in {"success", "failure", "cancelled", "skipped"}:
        raise ValueError("invalid Required result")
    return repository, sha.lower(), run_id, attempt, result


def existing_issue(repository, api):
    numbers = []
    for page in range(1, 6):
        issues = api("GET", f"repos/{repository}/issues?state=open&per_page=100&page={page}")
        if not isinstance(issues, list) or len(issues) > 100:
            raise RuntimeError("invalid issue list")
        for issue in issues:
            if (isinstance(issue, dict) and issue.get("title") == TITLE
                    and issue.get("state") == "open" and "pull_request" not in issue):
                number = issue.get("number")
                if type(number) is not int or number < 1:
                    raise RuntimeError("invalid issue number")
                numbers.append(number)
        if len(issues) < 100:
            return min(numbers) if numbers else None
    raise RuntimeError("nightly issue search exceeded bounded pages")


def main(env=os.environ, api=gh_api):
    repository, sha, run_id, attempt, result = validated_context(env)
    if result == "success":
        return
    body = (f"Required result: {result}\n"
            f"Run: https://github.com/{repository}/actions/runs/{run_id}/attempts/{attempt}\n"
            f"Source SHA: {sha}\n")
    number = existing_issue(repository, api)
    if number is None:
        api("POST", f"repos/{repository}/issues", {"title": TITLE, "body": body})
    else:
        api("PATCH", f"repos/{repository}/issues/{number}", {"body": body})


if __name__ == "__main__":
    main()

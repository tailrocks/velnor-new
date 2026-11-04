# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import re
import sys


def admitted(candidate, number, response, rules):
    if not re.fullmatch(r"[0-9a-f]{40}", candidate):
        return False
    if not re.fullmatch(r"[1-9][0-9]*", number):
        return False
    try:
        pr = response["data"]["repository"]["pullRequest"]
        threads = pr["reviewThreads"]
        commit = pr["commits"]["nodes"][0]["commit"]
        rollup = commit["statusCheckRollup"]
        contexts = rollup["contexts"]
        required = [rule["parameters"] for rule in rules if rule["type"] == "required_status_checks"]
        reviews = [rule["parameters"] for rule in rules if rule["type"] == "pull_request"]
        return (
            not response.get("errors")
            and pr["number"] == int(number)
            and pr["state"] == "OPEN"
            and pr["isDraft"] is False
            and pr["baseRefName"] == "main"
            and pr["baseRepository"]["nameWithOwner"] == "tailrocks/velnor-new"
            and pr["headRepository"]["nameWithOwner"] == "tailrocks/velnor-new"
            and pr["headRefOid"] == candidate
            and pr["mergeStateStatus"] == "CLEAN"
            and threads["pageInfo"]["hasNextPage"] is False
            and all(thread["isResolved"] is True for thread in threads["nodes"])
            and commit["oid"] == candidate
            and rollup["state"] == "SUCCESS"
            and contexts["pageInfo"]["hasNextPage"] is False
            and any(
                node.get("__typename") == "CheckRun"
                and node.get("name") == "Required"
                and node.get("conclusion") == "SUCCESS"
                and node.get("checkSuite", {}).get("app", {}).get("databaseId") == 15368
                and node.get("checkSuite", {}).get("commit", {}).get("oid") == candidate
                and node.get("checkSuite", {}).get("workflowRun", {}).get("file", {}).get("path") == ".github/workflows/ci.yml"
                and node.get("checkSuite", {}).get("workflowRun", {}).get("file", {}).get("repositoryName") == "tailrocks/velnor-new"
                for node in contexts["nodes"]
            )
            and any(
                rule["strict_required_status_checks_policy"] is True
                and any(check["context"] == "Required" for check in rule["required_status_checks"])
                for rule in required
            )
            and any(rule["required_review_thread_resolution"] is True for rule in reviews)
        )
    except (KeyError, TypeError, IndexError, ValueError, AttributeError):
        return False


def main():
    candidate, number, response_path, rules_path = sys.argv[1:]
    with open(response_path, encoding="utf-8") as response_file:
        response = json.load(response_file)
    with open(rules_path, encoding="utf-8") as rules_file:
        rules = json.load(rules_file)
    if not admitted(candidate, number, response, rules):
        raise SystemExit("candidate PR admission failed")


if __name__ == "__main__":
    main()

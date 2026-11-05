# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import json
import os
import subprocess

with open("/etc/os-release", encoding="utf-8") as info:
    release = info.read().splitlines()
if "ID=ubuntu" not in release or 'VERSION_ID="24.04"' not in release:
    raise SystemExit("unexpected operating system image")
result = subprocess.run(
    ["cargo", "nextest", "run", "--locked", "-p", "velnor-actions-workflow-renderer", "--test",
     "verified_gh_shell_public", "-E", "test(procfs_config_custody_rejects_same_user_poisoning)",
     "--message-format", "libtest-json"],
    env={**os.environ, "NEXTEST_EXPERIMENTAL_LIBTEST_JSON": "1"},
    capture_output=True, timeout=780, check=False,
)
if result.returncode != 0 or len(result.stdout) > 65536:
    raise SystemExit(f"hosted procfs GitHub CLI custody test failed: nextest_status={result.returncode} stdout_bytes={len(result.stdout)}")
def unique(pairs):
    output = {}
    for key, value in pairs:
        if key in output:
            raise ValueError("duplicate result key")
        output[key] = value
    return output

try:
    records = [json.loads(line, object_pairs_hook=unique) for line in result.stdout.splitlines()]
    target = "velnor-actions-workflow-renderer::verified_gh_shell_public$procfs_config_custody_rejects_same_user_poisoning"
    if (len(records) != 4 or [(record.get("type"), record.get("event")) for record in records]
            != [("suite", "started"), ("test", "started"), ("test", "ok"), ("suite", "ok")]
            or type(records[0].get("test_count")) is not int or records[0].get("test_count") != 1
            or records[1].get("name") != target
            or records[2].get("name") != target or records[3].get("passed") != 1
            or type(records[3].get("passed")) is not int or records[3].get("failed") != 0
            or type(records[3].get("failed")) is not int):
        raise ValueError("unexpected results")
except (ValueError, TypeError, AttributeError):
    raise SystemExit("hosted procfs GitHub CLI custody test failed") from None
evidence = {
    "schema": 1,
    "source": "mise-task-v1",
    "head": os.environ["VELNOR_CHECK_HEAD"],
    "check_id": os.environ["VELNOR_CHECK_ID"],
    "platform": os.environ["VELNOR_CHECK_PLATFORM"],
    "scenarios": [
        {"id": "procfs-gh-custody", "executed": True, "status": "passed"},
        {"id": "ubuntu-24.04", "executed": True, "status": "passed"},
    ],
}
with open("hosted-gh-config-evidence.json", "x", encoding="utf-8") as output:
    json.dump(evidence, output, separators=(",", ":"))

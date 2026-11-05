# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import json
import os

with open("/etc/os-release", encoding="utf-8") as info:
    release = info.read().splitlines()
if "ID=ubuntu" not in release or 'VERSION_ID="24.04"' not in release:
    raise SystemExit("unexpected operating system image")
readonly = bool(os.statvfs("/sys").f_flag & os.ST_RDONLY)
config_absent = all(not os.path.lexists(os.path.join("/sys", name)) for name in ("config.yml", "hosts.yml"))
if not readonly or not config_absent:
    raise SystemExit(f"hosted /sys isolation failed: readonly={readonly} config_absent={config_absent}")
evidence = {
    "schema": 1,
    "source": "mise-task-v1",
    "head": os.environ["VELNOR_CHECK_HEAD"],
    "check_id": os.environ["VELNOR_CHECK_ID"],
    "platform": os.environ["VELNOR_CHECK_PLATFORM"],
    "scenarios": [
        {"id": "sys-config-isolated", "executed": True, "status": "passed"},
        {"id": "ubuntu-24.04", "executed": True, "status": "passed"},
    ],
}
with open("hosted-gh-config-evidence.json", "x", encoding="utf-8") as output:
    json.dump(evidence, output, separators=(",", ":"))

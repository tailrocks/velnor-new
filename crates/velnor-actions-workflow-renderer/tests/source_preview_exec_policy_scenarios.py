# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
keys = json.loads(sys.argv[2])
sys.path.insert(0, str(root / "../velnor-actions-mise/src/source_preview"))
spec = importlib.util.spec_from_file_location("source_preview_exec", sys.argv[3])
executor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(executor)
environment = {key: "canonical-tool-version" for key in keys}
environment.update({"VELNOR_INTERNAL_OP": "resolve-task-execution-v1", "VELNOR_TOOL_SECRET": "secret", "VELNOR_REPO_POLICY_SECRET": "secret", "GH_TOKEN": "secret", "GITHUB_TOKEN": "secret", "ACTIONS_RUNTIME_TOKEN": "secret", "ACTIONS_ID_TOKEN_REQUEST_TOKEN": "secret"})
actual = executor.restricted_environment(environment, {"channel": "source-preview"})
for key in keys:
    assert actual.get(key) == "canonical-tool-version", "canonical project tool input was dropped: " + key
for key in ("VELNOR_TOOL_SECRET", "VELNOR_REPO_POLICY_SECRET", "GH_TOKEN", "GITHUB_TOKEN", "ACTIONS_RUNTIME_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN"):
    assert key not in actual, "unrelated secret reached the task resolver: " + key

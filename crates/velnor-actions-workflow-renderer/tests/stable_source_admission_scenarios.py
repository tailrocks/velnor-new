# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import runpy
import sys
import tempfile
from unittest.mock import patch

root = Path(sys.argv[1]).resolve()
script = root / "scripts/generator-release/stable-source-admission.py"
with tempfile.TemporaryDirectory() as directory:
    work = Path(directory)
    (work / "crates/velnor-runner").mkdir(parents=True)
    (work / "Cargo.lock").write_text("root-lock")
    (work / "crates/velnor-runner/Cargo.lock").write_text("runner-lock")
    output = work / "output"
    source = "a" * 40
    digest = "b" * 64
    env = {
        "GITHUB_SHA": source, "GITHUB_WORKFLOW_SHA": source, "GITHUB_RUN_ID": "37", "GITHUB_RUN_ATTEMPT": "2",
        "GITHUB_REPOSITORY": "tailrocks/velnor-new", "GITHUB_REPOSITORY_ID": "1390620900", "GITHUB_OUTPUT": str(output),
        "SOURCES_ID": "42", "SOURCES_DIGEST": digest, "SOURCES_TREE": "c" * 64,
        "SOURCES_ROOT_LOCK": hashlib.sha256(b"root-lock").hexdigest(),
        "SOURCES_RUNNER_LOCK": hashlib.sha256(b"runner-lock").hexdigest(),
        "SOURCES_ATTEMPT": "2", "SOURCES_NAME": "generator-linux-assets", "GH_TOKEN": "fixture",
    }
    artifact = {"id": 42, "name": "generator-linux-assets-sources-2", "expired": False,
                "digest": "sha256:" + digest, "size_in_bytes": 100,
                "workflow_run": {"id": 37, "head_sha": source, "repository_id": 1390620900}}
    previous = os.getcwd()
    os.chdir(work)
    try:
        def check(changes=None, metadata=None, expected=False):
            output.unlink(missing_ok=True)
            with patch.dict(os.environ, env | (changes or {}), clear=True), \
                 patch("urllib.request.urlopen", return_value=io.BytesIO(json.dumps(artifact if metadata is None else metadata).encode())), \
                 contextlib.redirect_stdout(io.StringIO()):
                try:
                    runpy.run_path(str(script), run_name="__main__")
                except SystemExit as error:
                    assert error.code == 0
            assert (output.exists() and output.read_text() == "admitted=true\n") == expected
        check(expected=True)
        for changes in ({"SOURCES_ATTEMPT": "1"}, {"GITHUB_SHA": "d" * 40}, {"SOURCES_ID": "43"},
                        {"SOURCES_DIGEST": "d" * 64}, {"SOURCES_ROOT_LOCK": "d" * 64},
                        {"SOURCES_RUNNER_LOCK": "d" * 64}, {"GITHUB_RUN_ID": "38"}):
            check(changes)
        check(metadata=artifact | {"size_in_bytes": 536870913})
        check(metadata=[])
        check(metadata=artifact | {"workflow_run": []})
        check(metadata=artifact | {"workflow_run": {"id": 37, "head_sha": "d" * 40, "repository_id": 1390620900}})
        check(metadata=artifact | {"expired": True})
        check(metadata=artifact | {"name": "generator-linux-assets-sources-1"})
    finally:
        os.chdir(previous)

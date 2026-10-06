"""Offline boundary checks for the fixed Gradle policy writer."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def run(source, root):
    environment = dict(os.environ, RUNNER_TEMP=str(root),
                       GRADLE_USER_HOME=str(root / "velnor/native/gradle"),
                       VELNOR_GRADLE_OUTPUT_POLICY="fixed-policy")
    return subprocess.run(["/usr/bin/python3", "-I", "-c", source],
                          env=environment, capture_output=True, check=False)


source = sys.argv[1]
with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary).resolve()
    assert run(source, root).returncode == 0
    policy = root / "velnor/native/gradle/velnor-output-policy.init.gradle"
    assert policy.read_text(encoding="utf-8") == "fixed-policy"
    assert run(source, root).returncode != 0
with tempfile.TemporaryDirectory() as temporary, tempfile.TemporaryDirectory() as outside:
    root = Path(temporary).resolve()
    (root / "velnor").symlink_to(Path(outside).resolve(), target_is_directory=True)
    assert run(source, root).returncode != 0
    assert list(Path(outside).iterdir()) == []

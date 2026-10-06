//! Credential-free source preparation for candidate qualification.

use crate::Yaml;

use super::workflow_steps;

/// Prepare the exact candidate source inline before local composite resolution.
pub(super) fn qualification_step() -> Yaml {
    workflow_steps::bash_step(
        "Fetch exact public source without an action post hook",
        QUALIFICATION_SOURCE_PREPARE,
    )
}

/// Fetch this public repository at the exact run commit without an action post hook.
pub(super) const QUALIFICATION_SOURCE_PREPARE: &str = r#"set -eu
python3 - "$GITHUB_WORKSPACE" "$GITHUB_REPOSITORY" "$GITHUB_SHA" <<'PY'
import os
import re
import subprocess
import sys

workspace, repository, commit = sys.argv[1:]
if repository != "tailrocks/velnor-new" or re.fullmatch(r"[0-9a-f]{40}", commit) is None:
    raise SystemExit("qualification source identity is invalid")

environment = os.environ.copy()
for name in tuple(environment):
    if name.startswith("GIT_") or name in {
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "MISE_GITHUB_TOKEN",
    }:
        environment.pop(name, None)
environment.update(
    GIT_CONFIG_GLOBAL=os.devnull,
    GIT_CONFIG_NOSYSTEM="1",
    GIT_TERMINAL_PROMPT="0",
)

def run_git(*arguments, timeout):
    subprocess.run(
        ["git", *arguments],
        cwd=workspace,
        env=environment,
        check=True,
        timeout=timeout,
    )

run_git("init", "--quiet", timeout=10)
run_git(
    "-c",
    "credential.helper=",
    "fetch",
    "--quiet",
    "--depth=1",
    "--no-tags",
    f"https://github.com/{repository}.git",
    commit,
    timeout=60,
)
run_git("checkout", "--quiet", "--detach", "FETCH_HEAD", timeout=10)
head = subprocess.run(
    ["git", "rev-parse", "HEAD"],
    cwd=workspace,
    env=environment,
    check=True,
    capture_output=True,
    text=True,
    timeout=5,
).stdout.strip()
if head != commit:
    raise SystemExit("qualification source does not match the requested commit")
PY"#;

#[cfg(test)]
mod tests;

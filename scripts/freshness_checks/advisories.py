import shutil
import subprocess
import tomllib

from report import fail_row, info_row, pass_row


def run_advisories(root, with_advisories):
    try:
        with open(f"{root}/deny.toml", "rb") as handle:
            deny = tomllib.load(handle)
    except (OSError, tomllib.TOMLDecodeError) as err:
        fail_row("advisories", "deny.toml", f"unreadable ({err})")
        deny = None
    if deny is not None:
        check_deny_policy(deny)
    if with_advisories:
        run_live_advisories(root)
    else:
        info_row("advisories", "live scan",
                 "runs as the CI Cargo Deny job; --with-advisories runs it here")


def check_deny_policy(deny):
    advisories = deny.get("advisories")
    if not isinstance(advisories, dict):
        fail_row("advisories", "deny.toml", "[advisories] table missing")
    elif advisories.get("ignore", []):
        for ignored in advisories["ignore"]:
            fail_row("advisories", str(ignored),
                     "ignored advisory must be a policy exception instead")
    else:
        pass_row("advisories", "deny ignore list", "empty")


def run_live_advisories(root):
    cargo_deny = shutil.which("cargo-deny")
    if cargo_deny is None:
        fail_row("advisories", "live scan",
                 "--with-advisories requested but cargo-deny is not on PATH")
        return
    try:
        completed = subprocess.run(
            [cargo_deny, "check", "advisories"], cwd=root,
            capture_output=True, text=True, timeout=180)
    except (OSError, subprocess.TimeoutExpired) as err:
        fail_row("advisories", "live scan", f"tool failed ({err})")
        return
    if completed.returncode == 0:
        pass_row("advisories", "live scan",
                 "cargo deny check advisories: no findings")
    else:
        tail = (completed.stdout + completed.stderr)[-500:]
        fail_row("advisories", "live scan",
                 "cargo deny reported findings; run "
                 f"`cargo deny check advisories` for evidence: {tail!r}")

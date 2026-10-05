#!/usr/bin/python3
"""Local contract fixture for exact pinned-source fetch bookkeeping."""
import importlib.util
import os
import pathlib
import re
import subprocess
import tempfile

TASK = "/root/.velnor-generator-preview-20261005"
SCRIPT = TASK + "/jackin-source-fetch-exact-v2.py"
TOKEN = "b80f991d56c2"
EXPECTED_COMMIT = "b80f991d56c2c13f913bb444f9f05a36b16d37df"
EXPECTED_TREE = "15a71f1adebd75dc217aa3a67fd7e0b78a38b2b6"
EXPECTED_PARENT = "3bee4c7217837bc37b928840eb93e717cc2bbfbe"
EXPECTED_REPOSITORY = "https://github.com/jackin-project/jackin.git"


def check(condition, message):
    if not condition:
        raise SystemExit("fixture failure: " + message)


spec = importlib.util.spec_from_file_location("jackin_source_fetch_exact_v2", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

fd = module.task_fd()
try:
    profile = module.read_profile(fd, TOKEN)
finally:
    os.close(fd)
check(profile["origin"] == EXPECTED_REPOSITORY, "canonical repository origin")
check(profile["commit"] == EXPECTED_COMMIT, "exact source commit")
check(profile["tree"] == EXPECTED_TREE, "exact source tree")
check(profile["parent"] == EXPECTED_PARENT, "exact source parent")
check(profile["branch_tip_at_selection"] == EXPECTED_COMMIT, "selected branch tip binding")
check(module.SOURCE.endswith("/jackin-source-" + TOKEN + "-exact-v2"), "versioned source path")

git_env = {
    "HOME": "/nonexistent",
    "PATH": "/usr/bin:/bin",
    "LC_ALL": "C",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_SYSTEM": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_OPTIONAL_LOCKS": "0",
    "GIT_NO_REPLACE_OBJECTS": "1",
    "GIT_NO_LAZY_FETCH": "1",
    "GIT_ALLOW_PROTOCOL": "file",
}


def local_git(cwd, args, success=True):
    result = subprocess.run(
        ["/usr/bin/git", "-c", "core.hooksPath=/dev/null", *args],
        cwd=cwd,
        env=git_env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        close_fds=True,
        timeout=15,
        check=False,
    )
    if success and result.returncode != 0:
        raise SystemExit("fixture failure: local Git command failed: " + " ".join(args))
    return result


with tempfile.TemporaryDirectory(prefix="jackin-fetch-v2-", dir=TASK) as scratch:
    scratch_path = pathlib.Path(scratch)
    template = scratch_path / "empty-template"
    work = scratch_path / "work"
    origin = scratch_path / "origin.git"
    shallow = scratch_path / "shallow"
    template.mkdir(mode=0o700)
    local_git(scratch, ["init", "--template=" + str(template), "--initial-branch=main", str(work)])
    work_file = work / "payload.txt"
    work_file.write_text("base\n", encoding="utf-8")
    local_git(work, ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "add", "payload.txt"])
    local_git(work, ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--no-gpg-sign", "--no-verify", "-m", "fixture base"])
    (work / "payload.txt").write_text("selected snapshot\n", encoding="utf-8")
    local_git(work, ["add", "payload.txt"])
    local_git(work, ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--no-gpg-sign", "--no-verify", "-m", "fixture selected tip"])
    selected = local_git(work, ["rev-parse", "HEAD"]).stdout.decode("ascii").strip()
    selected_tree = local_git(work, ["rev-parse", "HEAD^{tree}"]).stdout.decode("ascii").strip()
    selected_parent = local_git(work, ["rev-parse", "HEAD^"]).stdout.decode("ascii").strip()
    local_git(scratch, ["init", "--bare", "--template=" + str(template), str(origin)])
    local_git(work, ["push", str(origin), "main"])
    local_git(scratch, ["init", "--template=" + str(template), "--initial-branch=main", str(shallow)])
    local_git(shallow, ["remote", "add", "origin", str(origin)])
    local_git(shallow, ["fetch", "--depth=1", "origin", selected])
    local_git(shallow, ["update-ref", "refs/heads/main", "FETCH_HEAD"])
    local_git(shallow, ["symbolic-ref", "HEAD", "refs/heads/main"])
    shallow_flag = local_git(shallow, ["rev-parse", "--is-shallow-repository"]).stdout.decode("ascii").strip()
    check(shallow_flag == "true", "fixture exercises a real shallow commit boundary")
    shallow_parent = local_git(shallow, ["rev-parse", "HEAD^"], success=False)
    check(shallow_parent.returncode != 0, "rev-parse cannot traverse a shallow boundary")
    raw_commit = local_git(shallow, ["cat-file", "commit", selected]).stdout
    parsed_tree, parsed_parent = module.parse_commit_object(raw_commit)
    check(parsed_tree == selected_tree, "raw shallow commit tree identity")
    check(parsed_parent == selected_parent, "raw shallow commit parent identity")

module.validate_exact_snapshot(EXPECTED_COMMIT, EXPECTED_COMMIT, EXPECTED_TREE, EXPECTED_PARENT)
for label, args in (
    ("mismatched source tip", (EXPECTED_COMMIT, "0" * 40, EXPECTED_TREE, EXPECTED_PARENT)),
    ("malformed tree", (EXPECTED_COMMIT, EXPECTED_COMMIT, "z" * 40, EXPECTED_PARENT)),
):
    try:
        module.validate_exact_snapshot(*args)
    except SystemExit:
        pass
    else:
        raise SystemExit("fixture failure: accepted " + label)

old_tip = "1" * 40
selected_commit = EXPECTED_COMMIT
new_tip = "2" * 40
observation = module.observe_branch_tips(old_tip, selected_commit, new_tip)
check(observation == {
    "advertised_tip_before_fetch": old_tip,
    "fetched_branch_tip": selected_commit,
    "advertised_tip_after_fetch": new_tip,
    "tip_stable_during_fetch": False,
}, "branch movement is recorded without substituting the exact commit")
try:
    module.observe_branch_tips(old_tip, "bad", new_tip)
except SystemExit:
    pass
else:
    raise SystemExit("fixture failure: accepted malformed live tip")

ref = module.parse_ls_remote((EXPECTED_COMMIT + "\t" + module.REMOTE_REF + "\n").encode("ascii"))
check(ref == EXPECTED_COMMIT, "advertised ref parsing")
try:
    module.parse_ls_remote(("0" * 40 + "\trefs/heads/other\n").encode("ascii"))
except SystemExit:
    pass
else:
    raise SystemExit("fixture failure: accepted unrelated advertised ref")

source = open(SCRIPT, "rt", encoding="utf-8").read()
check("\"--depth=1\", \"origin\", COMMIT" in source, "fetch uses exact requested object ID")
check("if branch_tip_before != COMMIT" not in source, "current branch tip does not replace pinned source")
check("\"fetched_branch_tip\": remote_commit" not in source, "live tip is recorded through validated observation")
check('"cat-file", "commit", COMMIT' in source, "main flow reads parent from the raw exact commit object")
check('"rev-parse", "HEAD^"' not in source, "main flow does not traverse a shallow parent")
print("PASS: exact source identity, separate moving ref, and raw parent parsing across a real depth-1 boundary")

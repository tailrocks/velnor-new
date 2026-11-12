# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import pathlib
import shutil
import signal
import subprocess
import sys
import tempfile

root = pathlib.Path(sys.argv[1]).resolve()
helper = root / "scripts/generator-release/cargo-source-handoff.sh"
cargo = subprocess.run(["rustup", "which", "cargo"], check=True, capture_output=True, text=True).stdout.strip()

def run(args, cwd, env, success=True):
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True)
    if (result.returncode == 0) != success:
        raise AssertionError(f"{args}: {result.returncode}\n{result.stdout}\n{result.stderr}")
    return result

def bounded_run(args, cwd, env, success, receipt=None):
    process = subprocess.Popen(args, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.communicate()
        if receipt is not None and (not receipt.is_file() or receipt.read_text() != "substituted"):
            raise AssertionError("FIFO substitution hook did not fire")
        raise AssertionError("restored Git FIFO blocked source handoff")
    if (process.returncode == 0) != success:
        raise AssertionError(f"Git FIFO handoff returned {process.returncode}\n{stdout}\n{stderr}")

with tempfile.TemporaryDirectory() as temporary:
    base = pathlib.Path(temporary).resolve()
    remote = base / "remote"
    (remote / "src").mkdir(parents=True)
    (remote / "Cargo.toml").write_text('[package]\nname="git-dependency"\nversion="0.1.0"\nedition="2021"\n')
    (remote / "src/lib.rs").write_text('pub fn answer() -> u8 { 42 }\n')
    env = {key: os.environ[key] for key in ("USER", "RUSTUP_HOME") if key in os.environ}
    env.setdefault("RUSTUP_HOME", str(pathlib.Path.home() / ".rustup"))
    for key, name in (("HOME", "home"), ("MISE_DATA_DIR", "mise-data"), ("MISE_CACHE_DIR", "mise-cache"), ("MISE_STATE_DIR", "mise-state"), ("MISE_CONFIG_DIR", "mise-config")):
        directory = base / name
        directory.mkdir(mode=0o700)
        env[key] = str(directory)
    env.update({"PATH": str(pathlib.Path(cargo).parent) + os.pathsep + os.environ.get("PATH", "/usr/bin:/bin"), "MISE_AUTO_INSTALL": "0", "MISE_EXEC_AUTO_INSTALL": "0", "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_TERMINAL_PROMPT": "0"})
    run(["git", "init", "-q"], remote, env)
    run(["git", "add", "."], remote, env)
    run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], remote, env)
    workspace = base / "workspace"
    for name in ("plain", "consumer"):
        (workspace / name / "src").mkdir(parents=True)
        (workspace / name / "src/lib.rs").write_text("pub fn marker() {}\n")
    (workspace / "Cargo.toml").write_text('[workspace]\nmembers=["plain","consumer"]\nresolver="2"\n')
    (workspace / "plain/Cargo.toml").write_text('[package]\nname="plain"\nversion="0.1.0"\nedition="2021"\n')
    (workspace / "consumer/Cargo.toml").write_text(f'[package]\nname="consumer"\nversion="0.1.0"\nedition="2021"\n[dependencies]\ngit-dependency={{git="{remote.as_uri()}"}}\n')
    runner_temp = base / "runner-temp"
    shared = runner_temp / "velnor/cargo"
    shared.mkdir(mode=0o700, parents=True)
    (shared / "registry/index").mkdir(parents=True)
    (shared / "registry/cache").mkdir()
    env.update({"CARGO_HOME": str(shared), "CARGO_TARGET_DIR": str(base / "initial-target")})
    run([cargo, "generate-lockfile", "--manifest-path", str(workspace / "Cargo.toml")], workspace, env)
    run([cargo, "fetch", "--locked", "--manifest-path", str(workspace / "Cargo.toml")], workspace, env)
    for item in (shared / "git/db").rglob("*"):
        if item.is_file() and item.stat().st_nlink > 1:
            replacement = item.with_name(item.name + ".owned-copy")
            shutil.copyfile(item, replacement)
            os.replace(replacement, item)
    database = next((shared / "git/db").iterdir())
    marker = base / "hook-executed"
    (database / "hooks").mkdir(exist_ok=True)
    (database / "foreign-canary").write_text("preserved")
    hook = database / "hooks/post-checkout"
    hook.write_text(f'#!/bin/sh\ntouch "{marker}"\n')
    hook.chmod(0o755)
    with (database / "config").open("a") as config:
        config.write(f'\n[core]\n hooksPath = {database / "hooks"}\n')
    (shared / "credentials.toml").write_text("private")
    (shared / "config.toml").write_text("untrusted")
    warm = base / "warm"
    warm.mkdir(mode=0o700)
    run(["bash", str(helper), "copy", str(shared), str(warm)], root, env)
    assert not (warm / "credentials.toml").exists()
    assert not (warm / "config.toml").exists()
    assert not (warm / "git/checkouts").exists()
    assert not marker.exists()
    offline = env | {"CARGO_HOME": str(warm), "CARGO_TARGET_DIR": str(base / "offline-target"), "CARGO_NET_OFFLINE": "true"}
    for package in ("plain", "consumer"):
        run([cargo, "build", "--locked", "--offline", "--manifest-path", str(workspace / "Cargo.toml"), "-p", package], workspace, offline)
    assert not marker.exists()
    race_home = base / "race-home"
    race_home.mkdir(mode=0o700)
    shutil.copytree(warm / "git", race_home / "git")
    race_db = next((race_home / "git/db").iterdir())
    race_file = next((race_db / "objects/pack").glob("*.pack"))
    race_receipt = base / "race-receipt"
    race_script = root / "crates/velnor-actions-workflow-renderer/tests/cargo_git_fifo_race.py"
    bounded_run([sys.executable, "-I", str(race_script), str(helper.parent / "cargo-git-handoff.py"), str(race_home), str(base), race_file.name, str(race_receipt)], root, env, False, race_receipt)
    assert race_receipt.read_text() == "substituted" and race_file.is_fifo()
    assert not marker.exists()
    fifo_cases = []
    for kind in ("loose", "pack"):
        fifo_home = base / f"fifo-{kind}"
        fifo_home.mkdir(mode=0o700)
        shutil.copytree(warm / "git", fifo_home / "git")
        database_copy = next((fifo_home / "git/db").iterdir())
        inspect = ["/usr/bin/python3", str(helper.parent / "cargo-git-handoff.py"), "inspect", str(fifo_home), str(base), "0", "0"]
        run(inspect, root, env)
        if kind == "loose":
            fifo = database_copy / "objects/ff" / ("f" * 38)
            fifo.parent.mkdir(exist_ok=True)
        else:
            fifo = next((database_copy / "objects/pack").glob("*.pack"))
            fifo.unlink()
        os.mkfifo(fifo, 0o600)
        bounded_run(inspect, root, env, False)
        assert fifo.is_fifo() and not marker.exists()
        fifo_cases.append((fifo_home, fifo.relative_to(fifo_home / "git")))
    empty = base / "empty"
    empty.mkdir(mode=0o700)
    run([cargo, "build", "--locked", "--offline", "--manifest-path", str(workspace / "Cargo.toml"), "-p", "plain"], workspace, offline | {"CARGO_HOME": str(empty)}, success=False)
    alternates = database / "objects/info/alternates"
    alternates.parent.mkdir(exist_ok=True)
    alternates.write_text(str(base / "foreign"))
    run(["bash", str(helper), "check", str(shared)], root, env, success=False)
    alternates.unlink()
    pack = next((database / "objects/pack").glob("*.pack"))
    hardlink = pack.with_name(pack.name + ".second")
    os.link(pack, hardlink)
    run(["bash", str(helper), "check", str(shared)], root, env, success=False)
    hardlink.unlink()
    original = pack.read_bytes()
    pack.write_bytes(b"corrupt")
    broken = base / "broken"
    broken.mkdir(mode=0o700)
    run(["bash", str(helper), "copy", str(shared), str(broken)], root, env)
    run([cargo, "build", "--locked", "--offline", "--manifest-path", str(workspace / "Cargo.toml"), "-p", "consumer"], workspace, offline | {"CARGO_HOME": str(broken), "CARGO_TARGET_DIR": str(base / "broken-target")}, success=False)
    pack.write_bytes(original)
    assert not marker.exists()
    (shared / "config.toml").unlink()
    (shared / "credentials.toml").unlink()
    nested = workspace / "crates/velnor-runner"
    (nested / "src").mkdir(parents=True)
    (nested / "src/lib.rs").write_text("pub fn nested() {}\n")
    (nested / "Cargo.toml").write_text(f'[package]\nname="nested"\nversion="0.1.0"\nedition="2021"\n[workspace]\n[dependencies]\ngit-dependency={{git="{remote.as_uri()}"}}\n')
    run([cargo, "generate-lockfile", "--manifest-path", str(nested / "Cargo.toml")], workspace, env)
    scripts = workspace / "scripts/generator-release"
    scripts.mkdir(parents=True)
    for name in ("cargo-source-handoff.sh", "cargo-git-handoff.py"):
        shutil.copyfile(helper.parent / name, scripts / name)
    evidence = runner_temp / "source-qualification-artifacts/logs"
    evidence.mkdir(parents=True)
    canary = runner_temp / "foreign-canary"
    canary.write_text("untouched")
    (shared / "registry/cache/foreign-canary").write_text("cached")
    before = [(workspace / "Cargo.lock").read_bytes(), (nested / "Cargo.lock").read_bytes()]
    prefetch_env = env | {"RUNNER_TEMP": str(runner_temp), "GITHUB_WORKSPACE": str(workspace), "GITHUB_SHA": "a" * 40, "VELNOR_RUST_VERSION": "1.99.1", "VELNOR_PREFETCH_RUST_ONLY": "1", "SOURCES_CACHE_MATCHED_KEY": "fixture", "SOURCES_CACHE_HIT": "true", "GIT_TEMPLATE_DIR": str(database / "hooks")}
    prefetch = root / "scripts/generator-release/source-qualification.sh"
    shutil.rmtree(shared / "git/checkouts", ignore_errors=True)
    run(["bash", str(prefetch), "prefetch-locked-sources-for-qualification-workspaces"], workspace, prefetch_env)
    assert not (shared / "git/db" / database.name / "hooks/post-checkout").exists(), "unsafe restored hook remained visible to Cargo"
    assert "hookspath" not in (shared / "git/db" / database.name / "config").read_text().lower(), "unsafe restored Git config remained visible to Cargo"
    quarantines = list(runner_temp.glob("velnor-git-quarantine.*"))
    assert len(quarantines) == 1
    assert (quarantines[0] / "original/db" / database.name / "hooks/post-checkout").is_file()
    assert (quarantines[0] / "original/db" / database.name / "foreign-canary").read_text() == "preserved"
    assert not marker.exists() and canary.read_text() == "untouched"
    for fifo_home, relative_fifo in fifo_cases:
        fifo_before = (fifo_home / "git" / relative_fifo).stat()
        shutil.rmtree(shared / "git")
        (fifo_home / "git").rename(shared / "git")
        bounded_run(["bash", str(prefetch), "prefetch-locked-sources-for-qualification-workspaces"], workspace, prefetch_env, True)
        log = (evidence / "cargo-prefetch.log").read_text()
        assert "manifest=Cargo.toml mode=network status=0" in log
        assert "manifest=crates/velnor-runner/Cargo.toml mode=network status=0" in log
        preserved = [item / "git" / relative_fifo for item in runner_temp.glob("velnor-cargo-quarantine.*") if (item / "git" / relative_fifo).is_fifo()]
        assert any((entry.stat().st_dev, entry.stat().st_ino) == (fifo_before.st_dev, fifo_before.st_ino) for entry in preserved)
        assert not marker.exists() and canary.read_text() == "untouched"
    shutil.rmtree(shared / "git/checkouts", ignore_errors=True)
    for item in (shared / "git/db").rglob("*"):
        if item.is_file() and item.stat().st_nlink > 1:
            original_bytes = item.read_bytes()
            replacement = item.with_name(item.name + ".owned-copy")
            shutil.copyfile(item, replacement)
            os.replace(replacement, item)
            assert item.read_bytes() == original_bytes
    (shared / "registry/index").mkdir(parents=True, exist_ok=True)
    (shared / "registry/cache").mkdir(exist_ok=True)
    run(["bash", str(helper), "check", str(shared)], root, env)
    current_pack = next((shared / "git/db").rglob("*.pack"))
    current_pack.chmod(0o600)
    current_pack.write_bytes(b"corrupt")
    run(["bash", str(prefetch), "prefetch-locked-sources-for-qualification-workspaces"], workspace, prefetch_env)
    log = (evidence / "cargo-prefetch.log").read_text()
    assert "restored sources discarded" in log
    assert "manifest=Cargo.toml mode=network status=0" in log
    assert "manifest=crates/velnor-runner/Cargo.toml mode=network status=0" in log
    assert any((item / "registry/cache/foreign-canary").is_file() for item in runner_temp.glob("velnor-cargo-quarantine.*"))
    assert [(workspace / "Cargo.lock").read_bytes(), (nested / "Cargo.lock").read_bytes()] == before
    assert not marker.exists() and canary.read_text() == "untouched"
    collision_home = base / "collision-home"
    collision_staging = base / "collision-staging"
    collision_quarantine = base / "collision-quarantine"
    for directory in (collision_home, collision_staging, collision_quarantine):
        directory.mkdir(mode=0o700)
    (collision_home / "git").mkdir()
    (collision_home / "git/canary").write_text("source")
    (collision_staging / "git").mkdir()
    (collision_quarantine / "original").write_text("foreign")
    run(["/usr/bin/python3", str(helper.parent / "cargo-git-handoff.py"), "install", str(collision_home), str(collision_staging), str(collision_quarantine)], root, env, success=False)
    assert (collision_home / "git/canary").read_text() == "source"
    assert (collision_quarantine / "original").read_text() == "foreign"

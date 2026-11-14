# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import gzip
import hashlib
import http.server
import io
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading

root = pathlib.Path(sys.argv[1]).resolve()
helper = root / "scripts/generator-release/cargo-source-handoff.sh"
prefetch = root / "scripts/generator-release/source-qualification.sh"
cargo = shutil.which("cargo")
mise = shutil.which("mise")
if not cargo or not mise:
    raise RuntimeError("cargo and mise are required")
rustup = shutil.which("rustup")
if rustup:
    cargo = subprocess.run([rustup, "which", "cargo"], check=True, text=True, capture_output=True).stdout.strip()
else:
    cargo = subprocess.run([mise, "--no-config", "--no-env", "--no-hooks", "which", "cargo", "--tool=rust@1.99.1"], check=True, text=True, capture_output=True).stdout.strip()

with tempfile.TemporaryDirectory() as temporary:
    base = pathlib.Path(temporary).resolve()
    def fixture_crate(name):
        archive = io.BytesIO()
        with tarfile.open(fileobj=archive, mode="w") as tar:
            for path, body in {
                f"{name}-1.0.0/Cargo.toml": f'[package]\nname="{name}"\nversion="1.0.0"\nedition="2021"\n'.encode(),
                f"{name}-1.0.0/src/lib.rs": b"pub fn answer() -> u8 { 42 }\n",
            }.items():
                entry = tarfile.TarInfo(path)
                entry.size = len(body)
                entry.mode = 0o644
                tar.addfile(entry, io.BytesIO(body))
        return gzip.compress(archive.getvalue(), mtime=0)

    crates = {name: fixture_crate(name) for name in ("handoff-dep", "runner-dep", "windows-only")}
    checksums = {name: hashlib.sha256(data).hexdigest() for name, data in crates.items()}
    checksum = checksums["handoff-dep"]
    events = []

    class Registry(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            events.append(self.path)
            if self.path == "/config.json":
                data = json.dumps({"dl": f"http://127.0.0.1:{self.server.server_port}/api/v1/crates"}).encode()
            else:
                data = None
                for name, crate in crates.items():
                    if self.path == f"/{name[:2]}/{name[2:4]}/{name}":
                        data = (json.dumps({"name": name, "vers": "1.0.0", "deps": [], "cksum": checksums[name], "features": {}, "yanked": False}) + "\n").encode()
                    elif self.path == f"/api/v1/crates/{name}/1.0.0/download":
                        data = crate
                if data is None:
                    self.send_error(404)
                    return
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, *args):
            pass

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Registry)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    checkout = base / "checkout"
    nested = checkout / "crates/velnor-runner"
    nested.mkdir(parents=True)
    (checkout / ".cargo").mkdir()
    (checkout / ".cargo/config.toml").write_text(f'[source.crates-io]\nreplace-with="fixture"\n[source.fixture]\nregistry="sparse+http://127.0.0.1:{server.server_port}/"\n')
    git_remote = base / "git-remote"
    git_remote.mkdir()
    subprocess.run(["git", "init", "-q", str(git_remote)], check=True)
    (git_remote / "src").mkdir()
    (git_remote / "src/lib.rs").write_text("pub fn git_answer() -> u8 { 42 }\n")
    (git_remote / "Cargo.toml").write_text('[package]\nname="handoff-git"\nversion="0.1.0"\nedition="2021"\n')
    subprocess.run(["git", "-C", str(git_remote), "add", "."], check=True)
    subprocess.run(["git", "-C", str(git_remote), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], check=True)
    for project, name, dependency in ((checkout, "handoff-root", "handoff-dep"), (nested, "handoff-nested", "runner-dep")):
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(f"pub fn answer() -> u8 {{ {dependency.replace('-', '_')}::answer() }}\n")
        extra = f'handoff-dep="=1.0.0"\nhandoff-git={{git="{git_remote.as_uri()}"}}\n' if project == nested else ""
        windows = '\n[target.\'cfg(windows)\'.dependencies]\nwindows-only="=1.0.0"\n' if project == nested else ''
        (project / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2021"\n[dependencies]\n{dependency}="=1.0.0"\n{extra}{windows}')
    owned_script = checkout / "scripts/generator-release/cargo-source-handoff.sh"
    owned_script.parent.mkdir(parents=True)
    shutil.copyfile(helper, owned_script)
    shutil.copyfile(root / "scripts/generator-release/cargo-git-handoff.py", owned_script.parent / "cargo-git-handoff.py")
    shared = base / "temp/velnor/cargo"
    shared.mkdir(mode=0o700, parents=True)
    env = {key: os.environ[key] for key in ("HOME", "USER", "RUSTUP_HOME", "MISE_DATA_DIR") if key in os.environ}
    env["PATH"] = str(pathlib.Path(cargo).parent) + os.pathsep + str(pathlib.Path(mise).parent) + os.pathsep + os.environ.get("PATH", "/usr/bin:/bin")
    env["MISE_AUTO_INSTALL"] = "0"
    env["MISE_CACHE_DIR"] = str(base / "mise-cache")
    env["MISE_STATE_DIR"] = str(base / "mise-state")
    env["CARGO_HOME"] = str(shared)
    env["CARGO_TARGET_DIR"] = str(base / "source-target")
    env["CARGO_HTTP_PROXY"] = ""
    seed = base / "lock-home"
    seed.mkdir()
    for project in (checkout, nested):
        subprocess.run([cargo, "generate-lockfile", "--manifest-path", str(project / "Cargo.toml")], env=env | {"CARGO_HOME": str(seed)}, cwd=checkout, check=True, capture_output=True)
    locks = [project / "Cargo.lock" for project in (checkout, nested)]
    before = [hashlib.sha256(path.read_bytes()).hexdigest() for path in locks]
    evidence = base / "temp/source-qualification-artifacts/logs"
    evidence.mkdir(parents=True)
    prefetch_env = env | {"RUNNER_TEMP": str(base / "temp"), "GITHUB_WORKSPACE": str(checkout), "GITHUB_SHA": "a" * 40, "VELNOR_RUST_VERSION": "1.99.1", "VELNOR_PREFETCH_RUST_ONLY": "1", "SOURCES_CACHE_HIT": "", "SOURCES_CACHE_MATCHED_KEY": ""}
    result = subprocess.run(["bash", str(prefetch), "prefetch-locked-sources-for-qualification-workspaces"], env=prefetch_env, cwd=checkout, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    identity = (base / "temp/source-qualification-artifacts/cargo-fetch-lock-identity.txt").read_text()
    assert "fetch_mode=Cargo.toml:network" in identity
    assert "fetch_mode=crates/velnor-runner/Cargo.toml:network" in identity
    assert events
    for object_file in (shared / "git/db").rglob("*"):
        if object_file.is_file() and object_file.stat().st_nlink > 1:
            data = object_file.read_bytes()
            object_file.unlink()
            object_file.write_bytes(data)
    consumer_temp = base / "consumer-temp"
    consumer_temp.mkdir(mode=0o700)
    staged = consumer_temp / "stable-cargo-download"
    staged.mkdir(mode=0o700)
    subprocess.run(["bash", str(helper), "copy", str(shared), str(staged)], cwd=checkout, env=env, check=True)
    for script in ("cargo-source-tree.py", "stable-source-consumer.sh", "source-qualification.sh"):
        shutil.copyfile(root / "scripts/generator-release" / script, owned_script.parent / script)
    tree_helper = owned_script.parent / "cargo-source-tree.py"
    consumer = owned_script.parent / "stable-source-consumer.sh"
    consumer_env = env | {"RUNNER_TEMP": str(consumer_temp), "GITHUB_WORKSPACE": str(checkout), "GITHUB_SHA": "a" * 40,
                          "GITHUB_OUTPUT": str(base / "consumer-output"), "GITHUB_ENV": str(base / "consumer-env"),
                          "VELNOR_RUST_VERSION": "1.99.1", "SOURCES_DOWNLOADED": "success", "CARGO_NET_RETRY": "0", "SOURCES_CACHE_HIT": "true", "SOURCES_CACHE_MATCHED_KEY": "inherited-fake", "CARGO_TARGET_DIR": str(base / "consumer-target")}
    nested_archive = next((staged / "registry/cache").rglob("runner-dep-1.0.0.crate"))
    saved_stage = base / "saved-stage"
    shutil.copytree(staged, saved_stage)
    valid_only = os.environ.get("STABLE_TEST_VALID_ONLY") == "1"
    if not valid_only:
        nested_archive.unlink()
    consumer_env["SOURCES_TREE"] = subprocess.run([sys.executable, str(tree_helper), str(staged)], check=True, text=True, capture_output=True).stdout.strip()
    server.shutdown()
    worker.join()
    server.server_close()
    git_remote.rename(base / "git-remote-offline")
    if not valid_only:
        broken = subprocess.run(["bash", str(consumer)], env=consumer_env, cwd=checkout, text=True, capture_output=True, timeout=60)
        assert broken.returncode != 0, "missing nested source passed a fresh offline consumer"
        assert not staged.exists()
        broken_log = (consumer_temp / "source-qualification-artifacts/logs/cargo-prefetch.log").read_text()
        assert "manifest=Cargo.toml mode=offline status=0" in broken_log
        assert "manifest=crates/velnor-runner/Cargo.toml mode=offline status=" in broken_log
        assert "runner-dep" in broken_log
        shutil.rmtree(consumer_temp / "velnor")
        shutil.copytree(saved_stage, staged)
    consumer_env["SOURCES_TREE"] = subprocess.run([sys.executable, str(tree_helper), str(staged)], check=True, text=True, capture_output=True).stdout.strip()
    fixed = subprocess.run(["bash", str(consumer)], env=consumer_env, cwd=checkout, text=True, capture_output=True, timeout=60)
    assert fixed.returncode == 0, fixed.stdout + fixed.stderr
    assert not staged.exists()
    consumer_log = (consumer_temp / "source-qualification-artifacts/logs/cargo-prefetch.log").read_text()
    consumer_identity = (consumer_temp / "source-qualification-artifacts/cargo-fetch-lock-identity.txt").read_text()
    assert "sources_cache_hit=\n" in consumer_identity
    assert "sources_cache_matched_key=\n" in consumer_identity
    assert "manifest=Cargo.toml mode=offline status=0" in consumer_log
    assert "manifest=crates/velnor-runner/Cargo.toml mode=offline status=0" in consumer_log
    for project in (checkout, nested):
        subprocess.run([cargo, "build", "--locked", "--offline", "--manifest-path", str(project / "Cargo.toml")], env=consumer_env | {"CARGO_HOME": str(consumer_temp / "velnor/cargo")}, cwd=checkout, check=True, capture_output=True)
    (base / "git-remote-offline").rename(git_remote)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", server.server_port), Registry)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    shutil.rmtree(consumer_temp / "velnor")
    shutil.copytree(saved_stage, staged)
    consumer_env["SOURCES_TREE"] = "0" * 64
    before_fallback = len(events)
    fallback = subprocess.run(["bash", str(consumer)], env=consumer_env, cwd=checkout, text=True, capture_output=True, timeout=60)
    assert fallback.returncode == 0, fallback.stdout + fallback.stderr
    assert not staged.exists()
    fallback_log = (consumer_temp / "source-qualification-artifacts/logs/cargo-prefetch.log").read_text()
    assert "manifest=Cargo.toml mode=network status=0" in fallback_log
    assert "manifest=crates/velnor-runner/Cargo.toml mode=network status=0" in fallback_log
    assert "/api/v1/crates/handoff-dep/1.0.0/download" in events[before_fallback:]
    assert "/api/v1/crates/runner-dep/1.0.0/download" in events[before_fallback:]
    assert "/api/v1/crates/windows-only/1.0.0/download" in events[before_fallback:]
    assert [hashlib.sha256(path.read_bytes()).hexdigest() for path in locks] == before
    shutil.rmtree(consumer_temp / "velnor")
    shutil.copytree(saved_stage, staged)
    canary = base / "external-canary"
    canary.write_text("unchanged")
    (staged / "registry/index/escape").symlink_to(canary)
    consumer_env["SOURCES_TREE"] = "1" * 64
    unsafe = subprocess.run(["bash", str(consumer)], env=consumer_env, cwd=checkout, text=True, capture_output=True, timeout=60)
    assert unsafe.returncode == 0, unsafe.stdout + unsafe.stderr
    assert canary.read_text() == "unchanged" and not staged.exists()
    unsafe_log = (consumer_temp / "source-qualification-artifacts/logs/cargo-prefetch.log").read_text()
    assert "manifest=Cargo.toml mode=network status=0" in unsafe_log
    assert "manifest=crates/velnor-runner/Cargo.toml mode=network status=0" in unsafe_log
    server.shutdown()
    worker.join()
    server.server_close()

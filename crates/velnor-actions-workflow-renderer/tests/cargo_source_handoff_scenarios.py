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

    crates = {name: fixture_crate(name) for name in ("handoff-dep", "runner-dep")}
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
    for project, name, dependency in ((checkout, "handoff-root", "handoff-dep"), (nested, "handoff-nested", "runner-dep")):
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(f"pub fn answer() -> u8 {{ {dependency.replace('-', '_')}::answer() }}\n")
        (project / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2021"\n[dependencies]\n{dependency}="=1.0.0"\n')
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
    archive_path = next((shared / "registry/cache").rglob("handoff-dep-1.0.0.crate"))
    archive_path.write_bytes(b"corrupt")
    shutil.rmtree(shared / "registry/src", ignore_errors=True)
    prefetch_env |= {"SOURCES_CACHE_HIT": "true", "SOURCES_CACHE_MATCHED_KEY": "fixture"}
    result = subprocess.run(["bash", str(prefetch), "prefetch-locked-sources-for-qualification-workspaces"], env=prefetch_env, cwd=checkout, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    assert "restored sources discarded" in result.stdout
    assert hashlib.sha256(archive_path.read_bytes()).hexdigest() == checksum
    assert [hashlib.sha256(path.read_bytes()).hexdigest() for path in locks] == before
    runner_path = next((shared / "registry/cache").rglob("runner-dep-1.0.0.crate"))
    shutil.rmtree(shared / "registry/src", ignore_errors=True)
    runner_path.unlink()
    root_inode = archive_path.stat().st_ino
    prefetch_env |= {"SOURCES_CACHE_HIT": "false", "SOURCES_CACHE_MATCHED_KEY": "older-fixture"}
    result = subprocess.run(["bash", str(prefetch), "prefetch-locked-sources-for-qualification-workspaces"], env=prefetch_env, cwd=checkout, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    assert "restored sources discarded" not in result.stdout
    assert archive_path.stat().st_ino == root_inode
    assert hashlib.sha256(runner_path.read_bytes()).hexdigest() == checksums["runner-dep"]
    for root_was_network in (False, True):
        shutil.rmtree(shared / "registry/src", ignore_errors=True)
        if root_was_network:
            archive_path.unlink()
        runner_path.write_bytes(b"corrupt")
        result = subprocess.run(["bash", str(prefetch), "prefetch-locked-sources-for-qualification-workspaces"], env=prefetch_env, cwd=checkout, text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)
        assert "restored sources discarded" in result.stdout
        log = (evidence / "cargo-prefetch.log").read_text()
        assert "manifest=crates/velnor-runner/Cargo.toml mode=offline status=" in log
        assert "manifest=crates/velnor-runner/Cargo.toml mode=network status=" in log
        assert log.count("manifest=Cargo.toml mode=network status=0") == (2 if root_was_network else 1)
        assert hashlib.sha256(archive_path.read_bytes()).hexdigest() == checksum
        assert hashlib.sha256(runner_path.read_bytes()).hexdigest() == checksums["runner-dep"]
        for project in (checkout, nested):
            subprocess.run([cargo, "fetch", "--locked", "--offline", "--manifest-path", str(project / "Cargo.toml")], env=env, cwd=checkout, check=True, capture_output=True)
        assert [hashlib.sha256(path.read_bytes()).hexdigest() for path in locks] == before
    (shared / "credentials.toml").write_text("secret")
    (shared / "config.toml").write_text("untrusted")
    (shared / "target").mkdir()
    warm = base / "guard-home"
    warm.mkdir(mode=0o700)
    subprocess.run(["bash", str(helper), "copy", str(shared), str(warm)], check=True)
    assert not any((warm / name).exists() for name in ("credentials.toml", "config.toml", "target", "registry/src", "git"))
    server.shutdown()
    worker.join()
    offline = env | {"CARGO_HOME": str(warm), "CARGO_TARGET_DIR": str(base / "guard-target"), "CARGO_NET_OFFLINE": "true"}
    subprocess.run([cargo, "fetch", "--locked", "--offline", "--manifest-path", str(checkout / "Cargo.toml")], env=offline, cwd=checkout, check=True, capture_output=True)
    subprocess.run([cargo, "build", "--locked", "--offline", "--manifest-path", str(checkout / "Cargo.toml")], env=offline, cwd=checkout, check=True, capture_output=True)
    subprocess.run([cargo, "fetch", "--locked", "--offline", "--manifest-path", str(nested / "Cargo.toml")], env=offline, cwd=checkout, check=True, capture_output=True)
    missing = base / "missing-home"
    missing.mkdir()
    cold = subprocess.run([cargo, "fetch", "--locked", "--offline", "--manifest-path", str(checkout / "Cargo.toml")], env=offline | {"CARGO_HOME": str(missing)}, cwd=checkout, capture_output=True)
    assert cold.returncode != 0
    assert [hashlib.sha256(path.read_bytes()).hexdigest() for path in locks] == before
    (shared / "registry/index/unsafe").symlink_to(base)
    rejected = subprocess.run(["bash", str(helper), "check", str(shared)], capture_output=True)
    assert rejected.returncode != 0
    (shared / "registry/index/unsafe").unlink()
    hardlink = shared / "registry/cache/unsafe-hardlink"
    os.link(archive_path, hardlink)
    assert subprocess.run(["bash", str(helper), "check", str(shared)], capture_output=True).returncode != 0
    hardlink.unlink()
    fifo = shared / "registry/cache/unsafe-fifo"
    os.mkfifo(fifo)
    assert subprocess.run(["bash", str(helper), "check", str(shared)], capture_output=True).returncode != 0
    fifo.unlink()
    huge = shared / "registry/cache/oversize"
    with huge.open("wb") as stream:
        stream.truncate(536870913)
    assert subprocess.run(["bash", str(helper), "check", str(shared)], capture_output=True).returncode != 0

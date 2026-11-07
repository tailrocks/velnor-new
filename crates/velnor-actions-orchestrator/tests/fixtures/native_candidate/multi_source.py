# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import pathlib
import subprocess
import sys

root = pathlib.Path(sys.argv[1]).resolve()
root.mkdir(parents=True, exist_ok=False)
names = [f"fixture_completion_{index:02}" for index in range(58)]
(root / ".gitignore").write_text("/target/\n")
(root / "Cargo.toml").write_text(
    "[workspace]\nresolver = '2'\nmembers = ["
    + ", ".join(repr(name) for name in names)
    + "]\n"
)
for name in names:
    package = root / name
    (package / "src").mkdir(parents=True)
    (package / "Cargo.toml").write_text(
        f"[package]\nname = '{name}'\nversion = '0.1.0'\n"
        "edition = '2021'\nlicense = 'MIT'\ndescription = 'Completion fixture'\n"
    )
    (package / "src/lib.rs").write_text("pub fn fixture() {}\n")
    with (package / "payload.bin").open("wb") as payload:
        for index in range(9 * 1024):
            block = bytearray()
            for part in range(32):
                block.extend(hashlib.sha256(f"{name}:{index}:{part}".encode()).digest())
            payload.write(block)

subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, check=True)
subprocess.run(["git", "init", "-q"], cwd=root, check=True)
subprocess.run(["git", "add", "."], cwd=root, check=True)
subprocess.run([
    "git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
    "-c", "commit.gpgsign=false", "commit", "-qm", "multi-package fixture",
], cwd=root, check=True)
archives = []
for name in names:
    subprocess.run([
        "cargo", "package", "--offline", "--locked", "--no-verify", "--package", name,
    ], cwd=root, check=True)
    path = root / "target/package" / f"{name}-0.1.0.crate"
    size = path.stat().st_size
    if not 0 < size < 10_000_000:
        raise ValueError(f"invalid archive size: {name}: {size}")
    archives.append({"name": name, "path": str(path), "size": size})
if sum(archive["size"] for archive in archives) <= 512 * 1024 * 1024:
    raise ValueError("fixture does not exceed the completion limit")
print(json.dumps({"source": str(root), "archives": archives}))

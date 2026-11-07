# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import pathlib
import subprocess
import sys
import tarfile
import tempfile


def main():
    with tempfile.TemporaryDirectory() as temporary:
        root = pathlib.Path(temporary)
        source = root / "source"
        (source / ".cargo").mkdir(parents=True)
        (source / ".cargo/config.toml").write_text('[build]\nrustc-wrapper = "/nonexistent-hostile-wrapper"\ntarget-dir = "hostile-target"\n')
        (source / "Cargo.toml").write_text('[package]\nname = "fixture"\nversion = "0.1.0"\nedition = "2021"\n')
        (source / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "fixture"\nversion = "0.1.0"\n')
        (source / "src").mkdir()
        (source / "src/lib.rs").write_text('pub fn fixture() {}\n')
        archive = root / "source.tar"
        with tarfile.open(archive, "w") as output:
            for path in sorted(source.rglob("*")):
                if path.is_file():
                    output.add(path, arcname=path.relative_to(source).as_posix(), recursive=False)
        cargo = subprocess.check_output(["rustup", "which", "cargo"], text=True).strip()
        rustc = subprocess.check_output(["rustup", "which", "rustc"], text=True).strip()
        subprocess.run([sys.executable, sys.argv[1], str(source), str(archive), "Cargo.toml", cargo,
                        rustc, str(root / "rustup"), str(root), str(root / "metadata.json")], check=True)
        if not (root / "metadata.json").is_file():
            raise ValueError("metadata was not produced")
        data = json.loads((root / "metadata.json").read_text())
        if "hostile-target" in data["target_directory"]:
            raise ValueError("source Cargo configuration affected metadata")


if __name__ == "__main__":
    main()

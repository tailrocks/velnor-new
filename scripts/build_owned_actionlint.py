#!/usr/bin/env python3
"""Build an exact native Actionlint candidate; no distribution admission."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
RECIPE = ROOT / "scripts/actionlint-owned-build-recipe.json"


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def git_object(kind, data):
    prefix = kind.encode() + b" " + str(len(data)).encode() + b"\0"
    return hashlib.sha1(prefix + data).digest()


def git_tree(directory):
    entries = []
    for path in directory.iterdir():
        if path.name == ".git":
            continue
        name = path.name.encode("utf-8")
        if path.is_symlink():
            raise ValueError("source symlinks are not qualified")
        if path.is_dir():
            mode, value, order = b"40000", git_tree(path), name + b"/"
        elif path.is_file():
            mode = b"100755" if path.stat().st_mode & 0o111 else b"100644"
            value, order = git_object("blob", path.read_bytes()), name
        else:
            raise ValueError("nonregular source entry")
        entries.append((order, mode + b" " + name + b"\0" + value))
    return git_object("tree", b"".join(value for _, value in sorted(entries)))


def verify_source(source, identity):
    if source.is_symlink() or not source.is_dir():
        raise ValueError("source must be a regular directory")
    patch = ROOT / identity["patch_path"]
    if patch.is_symlink() or sha256(patch.read_bytes()) != identity["patch_sha256"]:
        raise ValueError("qualified patch digest mismatch")
    if git_tree(source).hex() != identity["patched_git_tree"]:
        raise ValueError("exact patched source Git tree mismatch")
    expected = {"go.mod": identity["go_mod_sha256"],
                "go.sum": identity["go_sum_sha256"], **identity["license_files"]}
    for name, digest in expected.items():
        file = source / name
        if file.is_symlink() or not file.is_file() or sha256(file.read_bytes()) != digest:
            raise ValueError("source lock or license mismatch: " + name)


def run(argv, source, environment):
    return subprocess.run(argv, cwd=source, env=environment, check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          text=True).stdout


def native_target(recipe):
    goos = {"Linux": "linux", "Darwin": "darwin"}.get(platform.system())
    goarch = {"x86_64": "amd64", "aarch64": "arm64", "arm64": "arm64"}.get(
        platform.machine())
    if goos == "darwin":
        hardware_arm64 = subprocess.run(
            ["/usr/sbin/sysctl", "-n", "hw.optional.arm64"], check=True,
            capture_output=True, text=True).stdout.strip()
        if hardware_arm64 not in ("0", "1"):
            raise ValueError("unexpected Darwin hardware architecture evidence")
        translated = subprocess.run(
            ["/usr/sbin/sysctl", "-n", "sysctl.proc_translated"],
            capture_output=True, text=True)
        if translated.returncode == 0 and translated.stdout.strip() != "0":
            raise ValueError("translated Darwin execution is not native proof")
        if (hardware_arm64 == "1") != (goarch == "arm64"):
            raise ValueError("Darwin hardware and process architecture differ")
    for target, pair in recipe["targets"].items():
        if pair == [goos, goarch]:
            return target, goos, goarch
    raise ValueError("native host is not qualified by the recipe")


def compiler_identity(recipe, target, receipt_path):
    approved = recipe["compiler"]["approved_assets"].get(target)
    if approved is None:
        raise ValueError("approved exact Go toolchain asset missing for " + target)
    if receipt_path is None or receipt_path.is_symlink():
        raise ValueError("approved compiler receipt required")
    receipt = json.loads(receipt_path.read_bytes())
    if receipt != approved:
        raise ValueError("compiler asset receipt does not match closed recipe authority")
    expected_keys = {"asset_url", "archive_sha256", "compiler_binary_sha256",
                     "toolchain_tree_sha256", "go_version"}
    if set(approved) != expected_keys or approved["go_version"] != recipe["compiler"]["go_version"]:
        raise ValueError("invalid approved compiler asset record")
    for field in ("archive_sha256", "compiler_binary_sha256", "toolchain_tree_sha256"):
        digest = approved[field]
        if (not isinstance(digest, str) or len(digest) != 64 or digest == "0" * 64
                or any(char not in "0123456789abcdef" for char in digest)):
            raise ValueError("invalid approved compiler digest")
    return approved


def toolchain_tree(root):
    entries = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise ValueError("nonregular compiler toolchain entry")
        if path.is_file():
            name = path.relative_to(root).as_posix()
            mode = "100755" if path.stat().st_mode & 0o111 else "100644"
            entries.append([name, mode, sha256(path.read_bytes())])
    return sha256(json.dumps(entries, separators=(",", ":"), ensure_ascii=False).encode())


def build(source, output, recipe, recipe_bytes, compiler_receipt):
    verify_source(source, recipe["source"])
    target, goos, goarch = native_target(recipe)
    approved = compiler_identity(recipe, target, compiler_receipt)
    compiler = shutil.which("go")
    if compiler is None:
        raise ValueError("exact Go compiler missing")
    compiler = str(Path(compiler).resolve())
    compiler_digest = sha256(Path(compiler).read_bytes())
    if compiler_digest != approved["compiler_binary_sha256"]:
        raise ValueError("compiler executable differs from approved toolchain asset")
    goroot = Path(compiler).parent.parent
    if toolchain_tree(goroot) != approved["toolchain_tree_sha256"]:
        raise ValueError("full compiler toolchain differs from approved asset tree")
    with tempfile.TemporaryDirectory(prefix="velnor-actionlint-build-") as temp:
        work = Path(temp)
        local_source = work / "source"
        shutil.copytree(source, local_source, ignore=shutil.ignore_patterns(".git"))
        verify_source(local_source, recipe["source"])
        environment = dict(recipe["environment"], GOOS=goos, GOARCH=goarch, GOROOT=str(goroot),
                           HOME=str(work / "home"), GOCACHE=str(work / "cache"),
                           GOPATH=str(work / "gopath"), GOMODCACHE=str(work / "modules"),
                           PATH=str(Path(compiler).parent) + os.pathsep + os.defpath,
                           TMPDIR=str(work))
        version = run([compiler, "version"], local_source, environment).strip()
        expected = "go version " + recipe["compiler"]["go_version"] + " " + goos + "/" + goarch
        if version != expected:
            raise ValueError("native compiler identity mismatch: " + version)
        for key in ("module_preparation_argv", "module_verify_argv"):
            run([compiler, *recipe[key][1:]], local_source, environment)
        verify_source(local_source, recipe["source"])
        environment.update(recipe["build_environment_override"])
        candidate = work / "actionlint"
        argv = [compiler if arg == "go" else str(candidate) if arg == "<output>/actionlint"
                else arg for arg in recipe["build_argv"]]
        run(argv, local_source, environment)
        verify_source(local_source, recipe["source"])
        banner = run([str(candidate), "-version"], local_source, environment)
        if banner != recipe["version_banner_template"].format(goos=goos, goarch=goarch):
            raise ValueError("owned version banner mismatch")
        if sha256(Path(compiler).read_bytes()) != compiler_digest:
            raise ValueError("compiler bytes changed during build")
        if toolchain_tree(goroot) != approved["toolchain_tree_sha256"]:
            raise ValueError("compiler toolchain changed during build")
        emit(output, candidate, recipe, recipe_bytes, target, goos, goarch, banner,
             compiler_digest, approved)


def emit(output, candidate, recipe, recipe_bytes, target, goos, goarch, banner,
         compiler_digest, approved):
    binary = candidate.read_bytes()
    receipt = {"schema": 1, "status": "native-build-candidate", "tool": "actionlint",
               "version": recipe["reported_version"], "version_banner": banner,
               "target": target, "source": recipe["source"],
               "compiler": {"go_version": recipe["compiler"]["go_version"],
                            "goos": goos, "goarch": goarch},
               "compiler_binary_sha256": compiler_digest,
               "compiler_asset": approved,
               "native_execution": {"platform_system": platform.system(),
                                    "platform_machine": platform.machine(),
                                    "hardware_native": True},
               "recipe_sha256": sha256(recipe_bytes),
               "artifact": {"binary_sha256": sha256(binary)}}
    output.mkdir(parents=True, exist_ok=False)
    artifact = output / "actionlint"
    artifact.write_bytes(binary)
    artifact.chmod(0o755)
    (output / "build-receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"target": target, "output": str(output),
                      "binary_sha256": receipt["artifact"]["binary_sha256"]}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--compiler-receipt", type=Path)
    args = parser.parse_args()
    if args.output_dir.exists() or args.output_dir.is_symlink():
        raise ValueError("output directory must be absent")
    recipe_bytes = RECIPE.read_bytes()
    recipe = json.loads(recipe_bytes)
    build(args.source_root.absolute(), args.output_dir.absolute(), recipe, recipe_bytes,
          args.compiler_receipt)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, TypeError, KeyError, subprocess.CalledProcessError) as error:
        if isinstance(error, subprocess.CalledProcessError):
            print(error.stderr, end="", file=sys.stderr)
        raise SystemExit(str(error)) from error

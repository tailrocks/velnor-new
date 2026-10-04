"""Exact negative-corpus bindings and public Cargo artifact identity; no authority."""

import copy
import importlib.util
import json
from pathlib import Path
import shutil
import tomllib

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("negative_base", ROOT / "run.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)


def blueprint(path, expected):
    descriptor = BASE.artifact(path)
    BASE.require(descriptor["sha256"] == expected, "reviewed negative blueprint differs")
    value = json.loads(path.read_bytes())
    BASE.require(type(value.get("schema")) is int and value["schema"] == 1,
                 "negative blueprint schema differs")
    return value, descriptor


def fixture(root, spec):
    BASE.canonical(root)
    records = BASE.BIND.inventory(root)
    BASE.require(records == spec["files"] and
                 BASE.BIND.inventory_sha(records) == spec["inventory_sha256"],
                 "negative fixture inputs changed")
    return records


def successor(blueprint_value, case):
    if "successor" in case:
        return copy.deepcopy(blueprint_value["fixtures"][case["successor"]])
    spec = copy.deepcopy(blueprint_value["fixtures"][case["fixture"]])
    if case["id"] == "T09-config":
        changed = case["successor_bytes"]
        BASE.require(BASE.artifact(Path(changed["path"])) == changed, "reviewed configuration bytes differ")
        replacement = dict(path=case["mutation_path"], size=changed["size"], sha256=changed["sha256"])
        spec["files"] = sorted([item for item in spec["files"] if item["path"] != replacement["path"]]
                               + [replacement], key=lambda item: item["path"])
        spec["inventory_sha256"] = BASE.BIND.inventory_sha(spec["files"])
    return spec


def normal_write(workspace, target_spec, sources):
    expected = {item["path"] for item in target_spec["files"]}
    current = {item["path"] for item in BASE.BIND.inventory(workspace)}
    BASE.require(current.issubset(expected), "negative mutation would remove unreviewed source inputs")
    for item in target_spec["files"]:
        target = workspace / item["path"]
        if target.is_file() and BASE.digest(target) == item["sha256"]:
            continue
        source = sources[item["path"]]
        BASE.require(BASE.artifact(source)["sha256"] == item["sha256"], "mutation source digest differs")
        target.parent.mkdir(parents=True, exist_ok=True)
        # Ordinary writes, never copying old mtimes or Cargo fingerprints.
        shutil.copyfile(source, target)
    fixture(workspace, target_spec)


def configurations(workspace, environment, spec):
    allowed = {workspace / item["path"]: item for item in spec["files"]
               if item["path"] == ".cargo/config.toml"}
    paths = set()
    for ancestor in (workspace, *workspace.parents):
        paths.update((ancestor / ".cargo/config", ancestor / ".cargo/config.toml", ancestor / ".mbx.toml"))
    home, cargo = Path(environment["HOME"]), Path(environment["CARGO_HOME"])
    paths.update((cargo / "config", cargo / "config.toml", home / ".config/mbx/config.toml",
                  home / "Library/Application Support/mbx/config.toml"))
    result = []
    for path in sorted(paths):
        if path in allowed:
            data = BASE.artifact(path)
            BASE.require(data["sha256"] == allowed[path]["sha256"], "reviewed Cargo config differs")
            result.append(dict(path=str(path), status="reviewed", artifact=data))
        else:
            BASE.require(not path.exists() and not path.is_symlink(), "unreviewed configuration discovered")
            result.append(dict(path=str(path), status="absent"))
    return result


def public_rlib(stdout, workspace, owned_root):
    manifest = tomllib.loads((workspace / "Cargo.toml").read_text())
    package = manifest["package"]["name"]
    library = manifest.get("lib", {}).get("name", package.replace("-", "_"))
    artifacts = []
    with stdout.open("rb") as stream:
        while True:
            line = stream.readline(8 * 1024 * 1024 + 1)
            if not line:
                break
            BASE.require(len(line) <= 8 * 1024 * 1024, "public Cargo message exceeds bound")
            if not line.strip():
                continue
            value = json.loads(line)
            if value.get("reason") != "compiler-artifact":
                continue
            target = value.get("target", {})
            if target.get("name") != library or "lib" not in target.get("kind", []):
                continue
            BASE.require(value.get("manifest_path") == str(workspace / "Cargo.toml") and
                         target.get("src_path") == str(workspace / "src/lib.rs"),
                         "public Cargo artifact package source differs")
            for name in value.get("filenames", []):
                path = Path(name)
                if path.suffix == ".rlib":
                    BASE.canonical(path)
                    BASE.require(path.is_relative_to(owned_root), "public artifact escapes owned execution root")
                    artifacts.append(dict(package_id=value["package_id"], target=target,
                                          artifact=BASE.artifact(path)))
    BASE.require(len(artifacts) == 1, "exactly one actual current public library artifact required")
    return artifacts[0]


def context_inventory(root):
    BASE.canonical(root)
    result = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            import os
            result.append(dict(path=relative, kind="symlink", target=os.readlink(path)))
        elif path.is_file():
            result.append(dict(path=relative, kind="file", size=path.stat().st_size, sha256=BASE.digest(path)))
        else:
            BASE.require(path.is_dir(), "special observer context file forbidden")
    return result


def closed_options(arguments, additional=()):
    options = {"--output", "--expected-manifest-sha256", "--toolchain-root", "--registry-home",
               "--registry-archive", "--registry-source", "--mbx", "--mbx-sha256", "--cargo",
               "--cargo-sha256", "--rustc", "--rustc-sha256", "--mbx-source-receipt",
               "--mbx-source-receipt-sha256", "--compiler-source-receipt",
               "--compiler-source-receipt-sha256", "--prequalified-local", "--help", *additional}
    for argument in arguments:
        if argument.startswith("--"):
            BASE.require(argument.split("=", 1)[0] in options, "unknown or abbreviated option")

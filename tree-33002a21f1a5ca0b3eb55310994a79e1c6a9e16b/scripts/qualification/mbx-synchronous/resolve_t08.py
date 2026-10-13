#!/usr/bin/env python3
"""Private T08 bootstrap: genuine pinned Cargo creates its initial offline lock."""

import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import time
import tomllib
import uuid

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("resolver_base", ROOT / "run.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)


def reviewed(path, expected):
    descriptor = BASE.artifact(path)
    BASE.require(descriptor["sha256"] == expected, "reviewed bootstrap input digest differs")
    return json.loads(path.read_bytes()), descriptor


def registry_spec(receipt):
    records = receipt["archive_member_inventory"]
    return dict(name=receipt["package"]["name"], version=receipt["package"]["version"],
                archive_sha256=receipt["package"]["checksum"], files=records,
                inventory_sha256=BASE.BIND.inventory_sha(records),
                local_extraction_marker=receipt["local_extraction_marker"])


def initial_inputs(args):
    blueprint, descriptor = reviewed(args.blueprint, args.expected_blueprint_sha256)
    receipt, source_descriptor = reviewed(args.source_receipt, args.expected_source_receipt_sha256)
    declared = blueprint["source_receipts"]["itoa17"]
    BASE.require(source_descriptor["sha256"] == declared["sha256"], "blueprint/source receipt differs")
    fixture = blueprint["fixtures"]["t08-itoa17"]
    source = BASE.canonical(Path(fixture["root"]))
    BASE.require(BASE.BIND.inventory(source) == fixture["files"], "initial fixture inputs changed")
    BASE.require(not (source / "Cargo.lock").exists(), "T08 initial lock already exists")
    BASE.require(receipt["package"] == dict(name="itoa", version="1.0.17",
                 source="registry+https://github.com/rust-lang/crates.io-index",
                 checksum="92ecc6618181def0457392ccd0ee51198e065e016d1d527a7ac1b6dc7c1f09d2",
                 archive_url="https://static.crates.io/crates/itoa/itoa-1.0.17.crate"),
                 "unexpected T08 registry tuple")
    archive = BASE.canonical(args.registry_archive)
    extracted = BASE.canonical(args.registry_source)
    BASE.BIND.verify_registry(registry_spec(receipt), archive, extracted)
    index_root = args.registry_home / "registry/index" / extracted.parent.name
    index = index_root / ".cache/it/oa/itoa"
    actual = BASE.artifact(index)
    expected = receipt["registry_index"]["artifact"]
    BASE.require(all(actual[key] == expected[key] for key in ("size", "sha256")),
                 "reviewed registry index changed")
    BASE.require(BASE.artifact(index_root / "config.json")["sha256"] == args.expected_index_config_sha256,
                 "reviewed registry index configuration differs")
    args.archive_relative = archive.relative_to(args.registry_home)
    args.source_relative = extracted.relative_to(args.registry_home)
    return blueprint, fixture, receipt, descriptor, source_descriptor


def generated_lock(path, expected_package):
    data = tomllib.loads(BASE.BIND.regular(path).decode())
    BASE.require(type(data.get("version")) is int and data["version"] == 4
                 and type(data.get("package")) is list,
                 "generated Cargo lock schema differs")
    packages = data["package"]
    dependency = [item for item in packages if item.get("name") == "itoa"]
    BASE.require(len(dependency) == 1 and dependency[0] == dict(
        name=expected_package["name"], version=expected_package["version"],
        source=expected_package["source"], checksum=expected_package["checksum"]),
        "actual generated registry resolution differs")
    root = [item for item in packages if item.get("name") == "mbx-synchronous-registry-fixture"]
    BASE.require(len(root) == 1 and root[0] == dict(name="mbx-synchronous-registry-fixture",
                 version="0.0.0", dependencies=["itoa"]) and len(packages) == 2,
                 "generated lock contains an unreviewed package closure")
    return data


def execute_bootstrap(args, record):
    blueprint, fixture, receipt, blueprint_artifact, source_artifact = initial_inputs(args)
    record.update(blueprint=blueprint_artifact, registry_source_receipt=source_artifact,
                  tools={name: BASE.artifact(getattr(args, name)) for name in ("cargo", "rustc")})
    for name, tool in record["tools"].items():
        BASE.require(tool["sha256"] == getattr(args, name + "_sha256"), "owned bootstrap tool differs")
        BASE.require(getattr(args, name).parent == args.toolchain_root / "bin", "owned distribution bin required")
    distribution = BASE.tree(args.toolchain_root)
    distribution_receipt = BASE.write_json(args.output / "toolchain-inventory.json", distribution)
    BASE.require(distribution_receipt["sha256"] == args.expected_toolchain_inventory_sha256,
                 "reviewed bootstrap distribution differs")
    record["toolchain_inventory"] = distribution_receipt
    run = args.output / "bootstrap"
    run.mkdir()
    for name in ("home", "cargo-home", "rustup-home", "target", "cache", "tmp", "logs"):
        (run / name).mkdir()
    workspace = run / "workspace"
    shutil.copytree(Path(fixture["root"]), workspace)
    BASE.seed_registry(args, run / "cargo-home")
    environment = BASE.environment(args, run)
    record.update(cwd=str(workspace), environment=environment,
                  root_allocation=dict(device=run.stat().st_dev, inode=run.stat().st_ino))
    record["config_before"] = BASE.config_inputs(workspace, environment)
    args.bootstrap_guards.append(("config", lambda: BASE.config_inputs(workspace, environment),
                                  record["config_before"]))
    before = BASE.BIND.inventory(workspace)
    args.bootstrap_guards.append(("fixture-sources", lambda: [item for item in BASE.BIND.inventory(workspace)
                                  if item["path"] != "Cargo.lock"], before))
    registry = BASE.tree(run / "cargo-home/registry")
    args.bootstrap_guards.append(("copied-registry", lambda: BASE.tree(run / "cargo-home/registry"), registry))
    record["fixture_before"] = BASE.write_json(run / "fixture-before.json", before)
    record["registry_before"] = BASE.write_json(run / "registry-before.json", registry)
    version = BASE.execute([args.cargo, "--version", "--verbose"], workspace, environment,
                           run / "logs", "cargo-version")
    record["tool_version"] = version
    BASE.require(version["returncode"] == 0 and
                 Path(version["stdout"]["path"]).read_text().startswith("cargo 1.98.1 "),
                 "bootstrap requires genuine Cargo 1.98.1")
    started = time.monotonic_ns()
    command = BASE.execute([args.cargo, "generate-lockfile", "--offline"], workspace,
                           environment, run / "logs", "generate-lockfile")
    command["wall_ns"] = time.monotonic_ns() - started
    record["command"] = command
    # Initial lock creation is this explicitly scoped exception; builds remain locked.
    BASE.require(command["returncode"] == 0, "genuine offline lock creation failed")
    record["generated_lock"] = BASE.artifact(workspace / "Cargo.lock")
    record["resolved_packages"] = generated_lock(workspace / "Cargo.lock", receipt["package"])["package"]


def final_guards(args, record, comparisons):
    record["after_guards"] = []
    for name, operation, expected in comparisons:
        try:
            observed = operation()
            witness = BASE.write_json(args.output / ("after-" + name + ".json"), observed)
            record["after_guards"].append(dict(name=name, witness=witness,
                status="unchanged" if observed == expected else "changed"))
        except (ValueError, OSError, KeyError) as error:
            record["after_guards"].append(dict(name=name, status="unavailable", error=str(error)))


def bootstrap_with_guards(args, record):
    # Capture origin closure before any subprocess, including failures during setup.
    comparisons = [("origin-registry", lambda: BASE.tree(args.registry_home)),
                   ("distribution", lambda: BASE.tree(args.toolchain_root)),
                   ("blueprint", lambda: BASE.artifact(args.blueprint)),
                   ("source-receipt", lambda: BASE.artifact(args.source_receipt)),
                   ("cargo", lambda: BASE.artifact(args.cargo)),
                   ("rustc", lambda: BASE.artifact(args.rustc))]
    expectations = [(name, operation, operation()) for name, operation in comparisons]
    blueprint, _ = reviewed(args.blueprint, args.expected_blueprint_sha256)
    fixture_root = Path(blueprint["fixtures"]["t08-itoa17"]["root"])
    expectations.append(("origin-fixture", lambda: BASE.BIND.inventory(fixture_root),
                         BASE.BIND.inventory(fixture_root)))
    failure = None
    args.bootstrap_guards = []
    try:
        execute_bootstrap(args, record)
    except (ValueError, OSError, KeyError) as error:
        failure = error
        record["primary_error"] = str(error)
    finally:
        final_guards(args, record, expectations + args.bootstrap_guards)
    if failure is not None:
        raise failure
    BASE.require(all(item["status"] == "unchanged" for item in record["after_guards"]),
                 "bootstrap input/tool after guard failed")
    record["status"] = "generated-awaiting-independent-review"


def arguments():
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    for name in ("output", "blueprint", "source-receipt", "registry-home", "registry-archive", "registry-source", "toolchain-root", "cargo", "rustc"):
        parser.add_argument("--" + name, type=Path, required=True)
    for name in ("expected-blueprint-sha256", "expected-source-receipt-sha256",
                 "expected-toolchain-inventory-sha256", "expected-index-config-sha256", "cargo-sha256", "rustc-sha256"):
        parser.add_argument("--" + name, required=True)
    args = parser.parse_args()
    for name in ("blueprint", "source_receipt", "registry_home", "registry_archive", "registry_source", "toolchain_root", "cargo", "rustc"):
        BASE.canonical(getattr(args, name))
    BASE.require(args.output.is_absolute() and not args.output.exists()
                 and not args.output.is_symlink(), "new absolute private bootstrap root required")
    BASE.canonical(args.output.parent)
    BASE.require(not args.output.is_relative_to(ROOT.parents[2]), "bootstrap must be outside repository")
    args.group = "t08-bootstrap-" + uuid.uuid4().hex
    return args


def main():
    args = arguments()
    args.output.mkdir(mode=0o700)
    record = dict(schema=1, scope="t08-offline-initial-lock-bootstrap-v1", status="failed",
                  native_authority=None, production_admission=None,
                  exception="Initial offline lock creation only; measured tasks remain --locked --offline.")
    try:
        bootstrap_with_guards(args, record)
    except (ValueError, OSError, KeyError) as error:
        record["error"] = str(error)
    record["symlink_anomalies"] = [str(path) for path in sorted(args.output.rglob("*")) if path.is_symlink()]
    record["artifacts"] = [BASE.artifact(path) for path in sorted(args.output.rglob("*"))
                           if path.is_file() and not path.is_symlink()]
    BASE.write_json(args.output / "bootstrap.json", record)
    print(json.dumps(dict(status=record["status"], output=str(args.output), native_authority=None)))
    return 0 if record["status"] == "generated-awaiting-independent-review" else 1


if __name__ == "__main__":
    sys.exit(main())

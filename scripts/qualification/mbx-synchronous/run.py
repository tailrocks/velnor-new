#!/usr/bin/env python3
"""Retain three independent local MBX observations; never mint native authority."""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parent
# Qualified Cargo 1.98.1 source: crates/cargo-util/src/paths.rs, lines 800-813.
# https://raw.githubusercontent.com/rust-lang/cargo/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/crates/cargo-util/src/paths.rs
# Retained source-text SHA-256: 77ce5172eed1315ff302e112b2f87bec8ba43cd1bc5111394ea1815b1ac2b34e.
# Cargo cache metadata is seeded before the strict input inventory, never excluded.
CARGO_CACHE_TAG = (b"Signature: 8a477f597d28d172789f06886806bc55\n"
                   b"# This file is a cache directory tag created by cargo.\n"
                   b"# For information about cache directory tags see https://bford.info/cachedir/\n")
SPEC = importlib.util.spec_from_file_location("fixture_binding", ROOT / "bind_inputs.py")
BIND = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BIND)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with path.open("rb") as source:
        value = hashlib.file_digest(source, "sha256").hexdigest()
    return value


def canonical(path):
    require(path.is_absolute() and path == path.resolve(strict=True),
            "absolute canonical path required: " + str(path))
    return path


def artifact(path):
    canonical(path)
    require(path.is_file() and not path.is_symlink(), "regular artifact required")
    return dict(path=str(path), size=path.stat().st_size, sha256=digest(path))


def write_json(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, sort_keys=True, indent=2)
        output.write("\n")
    return artifact(path)


def tree(directory):
    canonical(directory)
    records = []
    for path in sorted(directory.rglob("*")):
        require(not path.is_symlink(), "distribution/source symlink forbidden: " + str(path))
        if path.is_file():
            records.append(dict(path=path.relative_to(directory).as_posix(),
                                size=path.stat().st_size, sha256=digest(path)))
        else:
            require(path.is_dir(), "special file forbidden")
    return records


def source_receipts(args):
    result = {}
    for name in ("mbx", "compiler"):
        path = getattr(args, name + "_source_receipt")
        expected = getattr(args, name + "_source_receipt_sha256")
        require((path is None) == (expected is None), "receipt path/hash must be paired")
        if path is None:
            require(args.prequalified_local, "source receipt required or --prequalified-local")
            result[name] = None
        else:
            result[name] = artifact(path)
            require(result[name]["sha256"] == expected, "source receipt digest differs")
    return result


def config_inputs(workspace, environment):
    paths = set()
    for ancestor in (workspace, *workspace.parents):
        paths.update((ancestor / ".cargo/config", ancestor / ".cargo/config.toml",
                      ancestor / ".mbx.toml"))
    home = Path(environment["HOME"])
    cargo = Path(environment["CARGO_HOME"])
    paths.update((cargo / "config", cargo / "config.toml",
                  home / ".config/mbx/config.toml",
                  home / "Library/Application Support/mbx/config.toml"))
    for path in paths:
        require(not path.exists() and not path.is_symlink(),
                "unreviewed configuration discovery input: " + str(path))
    return [dict(path=str(path), status="absent") for path in sorted(paths)]


def environment(args, run):
    home = run / "home"
    return dict(PATH=str(args.toolchain_root / "bin") + os.pathsep + "/usr/bin:/bin",
                HOME=str(home), CARGO_HOME=str(run / "cargo-home"),
                RUSTUP_HOME=str(run / "rustup-home"), RUSTC=str(args.rustc),
                CARGO=str(args.cargo), CARGO_TARGET_DIR=str(run / "target"),
                CARGO_NET_OFFLINE="true", CARGO_BUILD_JOBS="2",
                XDG_CONFIG_HOME=str(home / ".config"),
                XDG_CACHE_HOME=str(home / ".cache"),
                XDG_DATA_HOME=str(home / ".local/share"),
                TMPDIR=str(run / "tmp"), LANG="C", LC_ALL="C", TZ="UTC",
                MBX_CACHE_DIR=str(run / "cache"), MBX_GC_AUTO="0",
                MBX_CACHE_EXPORT_GROUP=args.group,
                MBX_SUMMARY="off", MBX_DISPLAY="plain")


def prepare(args, number, manifest):
    run = args.output / ("run-" + str(number))
    run.mkdir()
    for name in ("home", "cargo-home", "rustup-home", "target", "cache", "tmp", "logs"):
        (run / name).mkdir()
    workspace = run / "workspace"
    BIND.copy_fixture(BIND.fixture_source(manifest), workspace)
    seed_registry(args, run / "cargo-home")
    env = environment(args, run)
    configs = config_inputs(workspace, env)
    registry_inventory = write_json(run / "registry-inputs.json", tree(run / "cargo-home/registry"))
    archive = run / "cargo-home" / args.archive_relative
    source = run / "cargo-home" / args.source_relative
    return dict(id=number, cwd=str(workspace), environment=env, config_inputs=configs,
                registry_archive=str(archive), registry_source=str(source),
                registry_inputs=registry_inventory, commands=[], transport=[])


def bind(args, run, label):
    workspace = Path(run["cwd"])
    inputs = argparse.Namespace(expected_manifest_sha256=args.expected_manifest_sha256,
                                fixture_root=workspace,
                                registry_archive=Path(run["registry_archive"]),
                                registry_source=Path(run["registry_source"]))
    manifest, checksum = BIND.verify(inputs)
    require(tree(workspace.parent / "cargo-home/registry") ==
            json.loads(Path(run["registry_inputs"]["path"]).read_bytes()),
            "copied registry inputs changed")
    receipt = dict(status="fixture-inputs-verified", manifest_sha256=checksum,
                   fixture_root=str(workspace),
                   fixture_inventory_sha256=manifest["fixture"]["inventory_sha256"],
                   registry_archive_sha256=manifest["registry"]["archive_sha256"],
                   registry_inventory_sha256=manifest["registry"]["inventory_sha256"],
                   registry_inputs=run["registry_inputs"], native_authority=None)
    return write_json(workspace.parent / "logs" / (label + "-inputs.json"), receipt)


def execute(argv, cwd, env, logs, name):
    stdout = logs / (name + ".stdout")
    stderr = logs / (name + ".stderr")
    record = dict(argv=[str(value) for value in argv], cwd=str(cwd), environment=dict(env),
                  returncode=None)
    with stdout.open("xb") as out, stderr.open("xb") as err:
        # No deadline, kill, compile wrapper, or inherited environment.
        result = subprocess.run(record["argv"], cwd=cwd, env=env, stdout=out,
                                stderr=err, check=False)
    record.update(returncode=result.returncode, stdout=artifact(stdout), stderr=artifact(stderr))
    return record


def retain_reports(record, reports):
    # Public reports are root files; private nested ledgers stay opaque bytes.
    files = [path for path in sorted(reports.rglob("*")) if path.is_file()]
    record["report_artifacts"] = [artifact(path) for path in files if path.parent == reports]
    record["admission_artifacts"] = [artifact(path) for path in files if path.parent != reports]


def observed_command(args, run, command, label):
    workspace = Path(run["cwd"])
    logs = workspace.parent / "logs"
    before = bind(args, run, label + "-before")
    reports = workspace.parent / (label + "-reports")
    reports.mkdir(mode=0o700)
    require(reports.stat().st_uid == os.getuid() and reports.stat().st_mode & 0o077 == 0,
            "native report directory must be owned and private")
    env = dict(run["environment"], MBX_STATS_REPORT_DIR=str(reports),
               MBX_REPORT_CORRELATION_ID=args.group + ":" + str(run["id"]) + ":" + label)
    configs_before = config_inputs(workspace, env)
    require(configs_before == run["config_inputs"], "configuration discovery scope changed")
    require(command[0] == "cargo", "unexpected fixture command")
    record = execute([args.mbx, *command[1:]], workspace, env, logs, label)
    run["commands"].append(record)
    record["fixture_before"] = before
    record["config_inputs_before"] = configs_before
    retain_reports(record, reports)
    record["fixture_after"] = bind(args, run, label + "-after")
    record["config_inputs_after"] = config_inputs(workspace, env)
    retain_reports(record, reports)
    require(record["returncode"] == 0, label + " workload failed")
    require(record["report_artifacts"], label + " completed report missing")


def seed_registry(args, destination):
    paths = [args.archive_relative, args.source_relative]
    index = Path("registry/index") / args.source_relative.parent.name
    paths.extend((index / "config.json", index / ".cache/it/oa/itoa"))
    for relative in paths:
        source = args.registry_home / relative
        canonical(source)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_dir():
            shutil.copytree(source, target)
        else:
            require(not source.is_symlink(), "registry seed symlink forbidden")
            shutil.copyfile(source, target)
    # This is harness-authored metadata; its bytes remain bound like every input.
    with (destination / "registry/CACHEDIR.TAG").open("xb") as tag:
        tag.write(CARGO_CACHE_TAG)


def transport(args, run, operation, bundle, baseline, manifest):
    workspace = Path(run["cwd"])
    argv = [args.mbx, "cache", operation]
    if operation == "export":
        argv += [bundle, "--format", "directory", "--json", "--group", args.group,
                 "--compare", baseline]
    elif operation == "import":
        witness = args.bundle_witnesses[str(bundle)]
        require(artifact(Path(witness["path"])) == witness, "retained bundle witness changed")
        retained = json.loads(Path(witness["path"]).read_bytes())
        require(tree(bundle) == retained, "retained native bundle inventory changed")
        disposable = workspace.parent / "import-bundle"
        shutil.copytree(bundle, disposable)
        require(tree(disposable) == retained, "transport copy inventory differs")
        import_inventory = write_json(workspace.parent / "import-bundle-inventory.json", retained)
        argv += [disposable, "--comparison-state", baseline, "--json", "--",
                 *manifest["commands"][0][1:]]
    else:
        argv += [baseline, "--json"]
    record = execute(argv, workspace, run["environment"],
                     workspace.parent / "logs", operation)
    run["transport"].append(record)
    if operation == "import":
        record["input_bundle_inventory"] = import_inventory
        record["retained_bundle"] = str(bundle)
        record["consumed_copy_exists_after"] = disposable.exists()
    require(record["returncode"] == 0, "MBX cache " + operation + " failed")
    if operation != "export":
        require(baseline.is_file(), "comparison baseline missing")
        record["comparison_state"] = artifact(baseline)
        return bundle
    observed = json.loads(Path(record["stdout"]["path"]).read_bytes())
    require(type(observed.get("exported")) is bool, "native export status missing")
    record["export_observed"] = observed
    if not observed["exported"]:
        require(not bundle.exists(), "skipped export unexpectedly created bundle")
        return None
    require(bundle.is_dir() and tree(bundle), "empty MBX directory bundle")
    record["bundle_inventory"] = write_json(workspace.parent / "bundle-inventory.json", tree(bundle))
    args.bundle_witnesses[str(bundle)] = record["bundle_inventory"]
    return bundle


def inspect_tools(args, record):
    inventory = tree(args.toolchain_root)
    record["toolchain_inventory"] = write_json(args.output / "toolchain-inventory.json", inventory)
    record["tools"] = {name: artifact(getattr(args, name)) for name in ("mbx", "cargo", "rustc")}
    for name in ("mbx", "cargo", "rustc"):
        require(record["tools"][name]["sha256"] == getattr(args, name + "_sha256"),
                name + " executable digest differs")
    require(args.cargo.parent == args.toolchain_root / "bin" and
            args.rustc.parent == args.toolchain_root / "bin", "owned distribution bin required")
    return inventory


def tool_observations(args, record, run):
    cwd = Path(run["cwd"])
    logs = args.output / "tool-logs"
    logs.mkdir()
    record["tool_observations"] = []
    for name, extra in (("mbx", ["--version"]), ("cargo", ["--version", "--verbose"]),
                        ("rustc", ["--version", "--verbose"]),
                        ("rustc", ["--print", "sysroot"])):
        label = name + "-" + extra[0].strip("-")
        item = execute([getattr(args, name), *extra], cwd, run["environment"], logs, label)
        record["tool_observations"].append(item)
        require(item["returncode"] == 0, "tool identity probe failed")
    sysroot_log = Path(record["tool_observations"][-1]["stdout"]["path"])
    require(sysroot_log.read_text().strip() == str(args.toolchain_root), "compiler sysroot differs")


def run_all(args, record):
    manifest, checksum = BIND.verify(argparse.Namespace(
        expected_manifest_sha256=args.expected_manifest_sha256, fixture_root=None,
        registry_archive=args.registry_home / args.archive_relative,
        registry_source=args.registry_home / args.source_relative))
    record.update(manifest_sha256=checksum, source_receipts=source_receipts(args),
                  fixture_inventory_sha256=manifest["fixture"]["inventory_sha256"],
                  registry_archive_sha256=manifest["registry"]["archive_sha256"],
                  registry_inventory_sha256=manifest["registry"]["inventory_sha256"])
    initial_tools = inspect_tools(args, record)
    bundle = None
    args.bundle_witnesses = {}
    for number in range(1, 4):
        run = prepare(args, number, manifest)
        record["runs"].append(run)
        if number == 1:
            record["registry_seed_inventory"] = run["registry_inputs"]
            tool_observations(args, record, run)
        baseline = Path(run["cwd"]).parent / "comparison-state.json"
        operation = "comparison-state" if number == 1 else "import"
        transport(args, run, operation, bundle, baseline, manifest)
        for index, command in enumerate(manifest["commands"]):
            observed_command(args, run, command, "command-" + str(index + 1))
        candidate = Path(run["cwd"]).parent / "mbx-cache-bundle"
        exported = transport(args, run, "export", candidate, baseline, manifest)
        require(exported is not None or bundle is not None, "cold export produced no bundle")
        bundle = exported or bundle
    require(tree(args.toolchain_root) == initial_tools, "tool distribution changed during execution")
    for name, descriptor in record["tools"].items():
        require(artifact(getattr(args, name)) == descriptor, "executed tool changed")
    record["status"] = "observed"


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="new absolute isolated directory")
    parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--toolchain-root", type=Path, required=True)
    parser.add_argument("--registry-home", type=Path, required=True, help="offline Cargo registry seed")
    parser.add_argument("--registry-archive", type=Path, required=True)
    parser.add_argument("--registry-source", type=Path, required=True)
    for name in ("mbx", "cargo", "rustc"):
        parser.add_argument("--" + name, type=Path, required=True)
        parser.add_argument("--" + name + "-sha256", required=True)
    for name in ("mbx", "compiler"):
        parser.add_argument("--" + name + "-source-receipt", type=Path)
        parser.add_argument("--" + name + "-source-receipt-sha256")
    parser.add_argument("--prequalified-local", action="store_true",
                        help="explicit unknown source/native authority observation mode")
    args = parser.parse_args()
    for name in ("toolchain_root", "registry_home", "registry_archive", "registry_source",
                 "mbx", "cargo", "rustc"):
        canonical(getattr(args, name))
    require(args.output.is_absolute() and not args.output.exists(), "new absolute output required")
    canonical(args.output.parent)
    args.archive_relative = args.registry_archive.relative_to(args.registry_home)
    args.source_relative = args.registry_source.relative_to(args.registry_home)
    require(args.archive_relative.parts[0] == "registry" and
            args.source_relative.parts[0] == "registry", "registry seed paths required")
    args.group = "fixture-local-" + uuid.uuid4().hex
    return args


def main():
    args = arguments()
    args.output.mkdir(mode=0o700)
    record = dict(schema=1, scope="exact_synchronous_fixture_v1", execution_kind="local",
                  run_attempt_id=args.group, status="failed", runs=[], native_authority=None,
                  native_abi=None, hosted_t01_t03=None,
                  host=dict(system=platform.system(), release=platform.release(),
                            machine=platform.machine()),
                  limitations=["Local observations cannot qualify hosted T01-T03.",
                               "Opaque source receipts and report booleans confer no native authority."])
    try:
        require(platform.system() in ("Linux", "Darwin"), "config isolation supports Unix only")
        run_all(args, record)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        record["error"] = str(error)
    record["artifacts"] = [artifact(path) for path in sorted(args.output.rglob("*"))
                           if path.is_file() and ("logs" in str(path.parent) or
                                                  "-reports" in str(path.parent))]
    write_json(args.output / "execution.json", record)
    print(json.dumps(dict(status=record["status"], output=str(args.output), native_authority=None)))
    return 0 if record["status"] == "observed" else 1


if __name__ == "__main__":
    sys.exit(main())

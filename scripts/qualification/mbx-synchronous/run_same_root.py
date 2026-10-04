#!/usr/bin/env python3
"""Observe three fresh local physical states at one exclusively owned absolute root."""

import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("fixture_base_run", ROOT / "run.py")
BASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BASE)


def allocation(path):
    info = path.stat()
    return dict(path=str(path), device=info.st_dev, inode=info.st_ino)


def acquire_root(path, number):
    BASE.canonical(path.parent)
    BASE.require(not path.exists() and not path.is_symlink(), "active root already exists")
    path.mkdir(mode=0o700)
    state = dict(id=number, original_root=str(path), allocation=allocation(path),
                 pristine_entries=sorted(item.name for item in path.iterdir()),
                 pristine=True, destroyed=False)
    state["allocation"].update(nonce=uuid.uuid4().hex, created_ns=time.monotonic_ns())
    BASE.require(not state["pristine_entries"], "new active root is not empty")
    return state


def assert_owned(state):
    path = Path(state["original_root"])
    BASE.canonical(path)
    current = allocation(path)
    BASE.require(not path.is_symlink() and current["device"] == state["allocation"]["device"]
                 and current["inode"] == state["allocation"]["inode"], "active root ownership changed")


def observe_directories(run):
    root = Path(run["cwd"]).parent
    observations = {}
    for name in ("workspace", "cargo-home", "target", "cache", "home", "tmp", "rustup-home"):
        path = root / name
        record = allocation(path)
        record["initial_entries"] = sorted(item.relative_to(path).as_posix()
                                           for item in path.rglob("*"))
        if name not in ("workspace", "cargo-home"):
            BASE.require(not record["initial_entries"], "fresh mutable directory is not empty")
        observations[name] = record
    BASE.require(observations["cargo-home"]["initial_entries"] and
                 all(item == "registry" or item.startswith("registry/")
                     for item in observations["cargo-home"]["initial_entries"]),
                 "Cargo home contains unreviewed mutable state")
    return observations


def archive_state(state, destination):
    assert_owned(state)
    original = Path(state["original_root"])
    BASE.require(not destination.exists(), "retained state already exists")
    # Archival bytes are never restored as mutable execution directories.
    shutil.copytree(original, destination, symlinks=True)
    witnesses, links = [], []
    for path in sorted(original.rglob("*")):
        retained = destination / path.relative_to(original)
        if path.is_symlink():
            BASE.require(retained.is_symlink() and os.readlink(path) == os.readlink(retained),
                         "archived symlink differs")
            links.append(dict(path=str(path), target=os.readlink(path)))
        elif path.is_file():
            descriptor = BASE.artifact(path)
            saved = BASE.artifact(retained)
            BASE.require(descriptor["size"] == saved["size"] and
                         descriptor["sha256"] == saved["sha256"], "archived bytes differ")
            observed = allocation(path)
            witnesses.append(dict(**descriptor, original_device=observed["device"],
                                  original_inode=observed["inode"], retained=saved))
        else:
            BASE.require(path.is_dir(), "special state file forbidden")
    state.update(retained_root=str(destination), retained_artifacts=witnesses,
                 archive_symlinks=links, retained_ns=time.monotonic_ns())
    return {item["path"]: item for item in witnesses}


def annotate_descriptors(value, witnesses):
    if isinstance(value, dict):
        if {"path", "size", "sha256"}.issubset(value):
            observed = witnesses.get(value["path"])
            if observed is not None:
                BASE.require(value["size"] == observed["size"] and
                             value["sha256"] == observed["sha256"], "recorded original bytes changed")
                value["retained"] = observed["retained"]
        for child in list(value.values()):
            if child is not value.get("retained"):
                annotate_descriptors(child, witnesses)
    elif isinstance(value, list):
        for child in value:
            annotate_descriptors(child, witnesses)


def destroy_state(state):
    BASE.require("retained_ns" in state and "retained_artifacts" in state,
                 "cannot destroy unretained state")
    assert_owned(state)
    root = Path(state["original_root"])
    shutil.rmtree(root)
    BASE.require(not root.exists() and not root.is_symlink(), "active root removal incomplete")
    state.update(destroyed=True, destroyed_ns=time.monotonic_ns())


def retain_bundle(bundle, state, witnesses, args):
    source = str(bundle)
    inventory = args.bundle_witnesses[source]
    BASE.require(BASE.artifact(Path(inventory["path"])) == inventory, "bundle witness changed")
    root = Path(state["original_root"])
    saved = Path(state["retained_root"]) / bundle.relative_to(root)
    saved_inventory = witnesses[inventory["path"]]["retained"]
    BASE.require(BASE.tree(saved) == json.loads(Path(saved_inventory["path"]).read_bytes()),
                 "retained native bundle differs")
    args.bundle_witnesses[str(saved)] = saved_inventory
    return saved


def observed_state(args, number, manifest, record, bundle):
    state = acquire_root(args.active_root, number)
    record["same_root_states"].append(state)
    active_args = copy.copy(args)
    active_args.output = args.active_root
    run = None
    exported = None
    failure = None
    try:
        run = BASE.prepare(active_args, 1, manifest)
        run["id"] = number
        record["runs"].append(run)
        state["directories"] = observe_directories(run)
        state["bootstrap_inputs"] = BASE.bind(active_args, run, "bootstrap")
        if number == 1:
            record["registry_seed_inventory"] = run["registry_inputs"]
            BASE.tool_observations(args, record, run)
        baseline = Path(run["cwd"]).parent / "comparison-state.json"
        operation = "comparison-state" if number == 1 else "import"
        BASE.transport(active_args, run, operation, bundle, baseline, manifest)
        for index, command in enumerate(manifest["commands"]):
            BASE.observed_command(active_args, run, command, "command-" + str(index + 1))
        candidate = Path(run["cwd"]).parent / "mbx-cache-bundle"
        exported = BASE.transport(active_args, run, "export", candidate, baseline, manifest)
        BASE.require(exported is not None or bundle is not None, "cold export produced no bundle")
    except (ValueError, OSError, KeyError) as error:
        failure = error
    destination = args.output / ("state-" + str(number)) / "raw"
    destination.parent.mkdir()
    witnesses = archive_state(state, destination)
    # Output annotations never mutate native proof descriptors shared with transport.
    annotate_descriptors(state, witnesses)
    if run is not None:
        archived_run = copy.deepcopy(run)
        record["runs"][-1] = archived_run
        annotate_descriptors(archived_run, witnesses)
    if number == 1 and "registry_seed_inventory" in record:
        record["registry_seed_inventory"] = copy.deepcopy(record["registry_seed_inventory"])
        annotate_descriptors(record["registry_seed_inventory"], witnesses)
    if exported is not None:
        bundle = retain_bundle(exported, state, witnesses, active_args)
        args.bundle_witnesses = active_args.bundle_witnesses
    state["execution_fragment"] = BASE.write_json(
        destination.parent / "execution-fragment.json",
        dict(state=copy.deepcopy(state), run=record["runs"][-1] if run is not None else None,
             tools=record.get("tools"), source_receipts=record.get("source_receipts")))
    destroy_state(state)
    state["lifecycle_receipt"] = BASE.write_json(destination.parent / "lifecycle.json", state)
    if failure is not None:
        raise failure
    return bundle


def run_all(args, record):
    inputs = argparse.Namespace(expected_manifest_sha256=args.expected_manifest_sha256,
                                fixture_root=None,
                                registry_archive=args.registry_home / args.archive_relative,
                                registry_source=args.registry_home / args.source_relative)
    manifest, checksum = BASE.BIND.verify(inputs)
    record.update(manifest_sha256=checksum, source_receipts=BASE.source_receipts(args),
                  fixture_inventory_sha256=manifest["fixture"]["inventory_sha256"],
                  registry_archive_sha256=manifest["registry"]["archive_sha256"],
                  registry_inventory_sha256=manifest["registry"]["inventory_sha256"])
    initial_tools = BASE.inspect_tools(args, record)
    args.bundle_witnesses = {}
    bundle = None
    for number in range(1, 4):
        bundle = observed_state(args, number, manifest, record, bundle)
    BASE.require(BASE.tree(args.toolchain_root) == initial_tools, "tool distribution changed")
    for name, descriptor in record["tools"].items():
        BASE.require(BASE.artifact(getattr(args, name)) == descriptor, "owned tool changed")
    record["status"] = "observed"


def arguments():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--active-root", type=Path, required=True)
    active, remainder = parser.parse_known_args()
    original = sys.argv
    try:
        sys.argv = [original[0], *remainder]
        args = BASE.arguments()
    finally:
        sys.argv = original
    args.active_root = active.active_root
    BASE.require(args.active_root.is_absolute() and not args.active_root.exists()
                 and not args.active_root.is_symlink(), "new absolute active root required")
    BASE.canonical(args.active_root.parent)
    BASE.require(not args.output.is_relative_to(args.active_root) and
                 not args.active_root.is_relative_to(args.output), "durable and active roots must be disjoint")
    return args


def main():
    args = arguments()
    args.output.mkdir(mode=0o700)
    lock = args.active_root.with_name(args.active_root.name + ".ownership")
    record = dict(schema=1, scope="exact_synchronous_fixture_v1", execution_kind="local_same_root",
                  run_attempt_id=args.group, status="failed", runs=[], same_root_states=[],
                  active_root=str(args.active_root), native_authority=None, native_abi=None,
                  hosted_t01_t03=None,
                  host=dict(system=platform.system(), release=platform.release(), machine=platform.machine()),
                  limitations=["Three sequential fresh physical local states share one absolute root.",
                               "Only immutable supported MBX exports cross execution states.",
                               "Retained files establish local byte observations, never native authority."])
    acquired = False
    try:
        BASE.require(platform.system() in ("Linux", "Darwin"), "Unix config isolation required")
        lock.mkdir(mode=0o700)
        acquired = True
        record["exclusive_ownership"] = allocation(lock)
        record["exclusive_ownership"]["nonce"] = uuid.uuid4().hex
        run_all(args, record)
    except (ValueError, OSError, KeyError) as error:
        record["error"] = str(error)
    finally:
        if acquired and not args.active_root.exists() and not args.active_root.is_symlink():
            lock.rmdir()
            record["exclusive_ownership"]["released_ns"] = time.monotonic_ns()
    record["artifacts"] = [BASE.artifact(path) for path in sorted(args.output.rglob("*"))
                           if path.is_file() and not path.is_symlink() and
                           ("logs" in str(path.parent) or "-reports" in str(path.parent))]
    BASE.write_json(args.output / "execution.json", record)
    print(json.dumps(dict(status=record["status"], output=str(args.output), native_authority=None)))
    return 0 if record["status"] == "observed" else 1


if __name__ == "__main__":
    sys.exit(main())

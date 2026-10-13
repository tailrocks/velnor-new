#!/usr/bin/env python3
"""Genuine local negative cases; raw observer execution never extends native authority."""

import argparse
import copy
import json
from pathlib import Path
import shutil
import sys
import time
import uuid

import importlib.util

ROOT = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


INPUTS = load("negative_inputs", ROOT / "negative_inputs_v2.py")
BASE = INPUTS.BASE
V2 = load("negative_v2_route", ROOT / "run_v2.py")
PROTOCOL = load("negative_public_transport", ROOT / "cache_transport_v2.py")
LIFECYCLE = load("negative_local_lifecycle", ROOT / "run_same_root.py")


def measured(argv, cwd, environment, logs, name):
    started = time.monotonic_ns()
    value = BASE.execute(argv, cwd, environment, logs, name)
    value.update(wall_ns=time.monotonic_ns() - started, observed_root_process_count=1)
    return value


def input_receipt(args, run, spec, registry, name):
    workspace = Path(run["cwd"])
    records = INPUTS.fixture(workspace, spec)
    configs = INPUTS.configurations(workspace, run["environment"], spec)
    actual_registry = BASE.tree(workspace.parent / "cargo-home/registry")
    BASE.require(actual_registry == registry, "negative registry input changed")
    positive = json.loads((ROOT / "manifest.json").read_bytes())
    BASE.BIND.verify_registry(positive["registry"], Path(run["registry_archive"]),
                              Path(run["registry_source"]))
    value = dict(fixture_inventory_sha256=BASE.BIND.inventory_sha(records),
                 fixture_root=str(workspace), files=records, configurations=configs,
                 registry_inputs=run["registry_inputs"], native_authority=None)
    return BASE.write_json(workspace.parent / "logs" / (name + "-inputs.json"), value)


def library_command(args, run, spec, registry, argv, expected_exit):
    workspace = Path(run["cwd"])
    before = input_receipt(args, run, spec, registry, "before")
    resolution_argv = INPUTS.resolution_argv(args.cargo, workspace, argv)
    resolution = measured(resolution_argv, workspace, run["environment"],
                          workspace.parent / "logs", "cargo-resolution")
    run["resolution"] = dict(command=resolution, metadata=resolution["stdout"],
                             build_argv=argv, native_authority=None)
    BASE.require(resolution["returncode"] == 0, "fresh Cargo dependency resolution failed")
    json.loads(Path(resolution["stdout"]["path"]).read_bytes())
    reports = workspace.parent / "library-reports"
    reports.mkdir(mode=0o700)
    env = dict(run["environment"], MBX_STATS_REPORT_DIR=str(reports),
               MBX_REPORT_CORRELATION_ID=args.group + ":" + str(run["id"]) + ":library")
    value = measured([args.mbx, *argv[1:]], workspace, env, workspace.parent / "logs", "library")
    run["commands"].append(value)
    value["inputs_before"] = before
    BASE.retain_reports(value, reports)
    value["inputs_after"] = input_receipt(args, run, spec, registry, "after")
    if expected_exit == "nonzero":
        BASE.require(value["returncode"] != 0, "expected semantic compile failure did not occur")
    else:
        BASE.require(value["returncode"] == expected_exit, "negative library command failed")
    return value


def observer(args, run, blueprint, command, expected_stdout):
    BASE.require(command["returncode"] == 0, "failed library cannot supply current observer output")
    workspace = Path(run["cwd"])
    source = Path(blueprint["observer"]["path"])
    BASE.require(BASE.artifact(source)["sha256"] == blueprint["observer"]["sha256"],
                 "reviewed observer source differs")
    resolution = run.get("resolution")
    BASE.require(type(resolution) is dict and type(resolution.get("metadata")) is dict,
                 "fresh Cargo resolution record required before observer compilation")
    rlib = INPUTS.public_rlib(Path(command["stdout"]["path"]), workspace,
                              Path(run["environment"]["CARGO_TARGET_DIR"]),
                              Path(resolution["metadata"]["path"]), resolution["metadata"],
                              command["stdout"])
    staging = INPUTS.stage_artifacts(rlib, workspace.parent / "observer-inputs")
    root_name = Path(rlib["artifact"]["path"]).name
    staged_root = Path(staging["directory"]) / root_name
    binary = workspace.parent / "observer-binary"
    BASE.require(not binary.exists(), "observer must be newly compiled")
    env = dict(run["environment"], SDKROOT=str(args.sdk_root))
    argv = [args.rustc, source, "--edition=2024", "--extern",
            rlib["crate_name"] + "=" + str(staged_root),
            "-L", "dependency=" + staging["directory"], "-o", binary]
    compiled = measured(argv, workspace, env, workspace.parent / "logs", "observer-compile")
    run["observer"] = dict(scope="separate-local-raw-tool-verification", compiler=compiled,
                           library_artifact=rlib, dependency_staging=staging,
                           linker_process_count=None, linker_wall_ns=None,
                           effective_linker=None, managed_admission_attribution=None,
                           native_authority=None, source_ownership=None)
    BASE.require(compiled["returncode"] == 0, "actual observer compilation failed")
    INPUTS.validate_source_closure(rlib)
    expected_staging = {Path(item["path"]).name: item for item in staging["source_artifacts"]}
    INPUTS.staged_inventory(Path(staging["directory"]), expected_staging)
    run["observer"]["binary"] = BASE.artifact(binary)
    executed = measured([binary], workspace, env, workspace.parent / "logs", "observer-run")
    run["observer"]["execution"] = executed
    BASE.require(executed["returncode"] == 0 and
                 Path(executed["stdout"]["path"]).read_text() == expected_stdout,
                 "actual linked observer behavior differs")


def fixture_sources(blueprint, case, spec, successor_stage):
    origin = Path(spec["root"])
    sources = {item["path"]: origin / item["path"] for item in spec["files"]}
    if successor_stage and case["id"] == "T09-config":
        sources[case["mutation_path"]] = Path(case["successor_bytes"]["path"])
    return sources


def physical_stage(args, blueprint, case, number, spec, initial_spec, record, bundle):
    state = LIFECYCLE.acquire_root(args.active_root, number)
    record["states"].append(state)
    active = copy.copy(args)
    active.output = args.active_root
    positive = json.loads((ROOT / "manifest.json").read_bytes())
    run, exported, failure = None, None, None
    try:
        run = BASE.prepare(active, 1, positive)
        run["id"] = number
        record["runs"].append(run)
        state["directories"] = LIFECYCLE.observe_directories(run)
        if number == 1:
            BASE.tool_observations(args, record, run)
        workspace = Path(run["cwd"])
        INPUTS.normal_write(workspace, initial_spec, fixture_sources(blueprint, case, initial_spec, False))
        if number == 2:
            INPUTS.normal_write(workspace, spec, fixture_sources(blueprint, case, spec, True))
        registry = BASE.tree(workspace.parent / "cargo-home/registry")
        baseline = workspace.parent / "comparison-state.json"
        argv = case.get("successor_argv", case.get("argv")) if number == 2 else case.get("baseline_argv", case.get("argv"))
        selector = dict(commands=[argv])
        BASE.transport(active, run, "comparison-state" if number == 1 else "import", bundle, baseline, selector)
        expected_exit = case.get("expected_build_exit", 0) if number == 2 else 0
        command = library_command(active, run, spec, registry, argv, expected_exit)
        if expected_exit == "nonzero":
            text = Path(command["stdout"]["path"]).read_bytes() + Path(command["stderr"]["path"]).read_bytes()
            BASE.require(case["expected_diagnostic"].encode() in text, "expected compiler diagnostic absent")
            run["observer"] = None
            run["export_skipped_reason"] = "failed-current-library-command"
        else:
            expected = case.get("successor_stdout", case.get("expected_observer_stdout")) if number == 2 else case.get("baseline_stdout", blueprint["observer"]["baseline_stdout"])
            observer(active, run, blueprint, command, expected)
            exported = BASE.transport(active, run, "export", workspace.parent / "mbx-cache-bundle", baseline, selector)
            BASE.require(exported is not None or bundle is not None, "baseline native export missing")
    except (ValueError, OSError, KeyError) as error:
        failure = error
    retained = args.output / ("state-" + str(number)) / "raw"
    retained.parent.mkdir()
    witnesses = LIFECYCLE.archive_state(state, retained)
    if run is not None:
        record["runs"][-1] = copy.deepcopy(run)
        LIFECYCLE.annotate_descriptors(record["runs"][-1], witnesses)
    if exported is not None:
        bundle = LIFECYCLE.retain_bundle(exported, state, witnesses, active)
        args.bundle_witnesses = active.bundle_witnesses
    state["execution_fragment"] = BASE.write_json(retained.parent / "execution-fragment.json",
                                                  dict(state=copy.deepcopy(state), run=record["runs"][-1] if run else None))
    LIFECYCLE.destroy_state(state)
    state["lifecycle_receipt"] = BASE.write_json(retained.parent / "lifecycle.json", state)
    if failure is not None:
        raise failure
    return bundle


def execute_case(args, record, blueprint, case, initial, successor):
    BASE.BIND.verify(argparse.Namespace(expected_manifest_sha256=args.expected_manifest_sha256,
                     fixture_root=None, registry_archive=args.registry_home / args.archive_relative,
                     registry_source=args.registry_home / args.source_relative))
    tools = BASE.inspect_tools(args, record)
    record["source_receipts"] = BASE.source_receipts(args)
    sdk = INPUTS.context_inventory(args.sdk_root)
    sdk_receipt = BASE.write_json(args.output / "sdk-inventory.json", sdk)
    BASE.require(sdk_receipt["sha256"] == args.expected_sdk_inventory_sha256, "reviewed SDK context differs")
    record["observer_context"] = dict(sdk_root=str(args.sdk_root), sdk_inventory=sdk_receipt,
                                      inspected_linker=BASE.artifact(args.linker) if args.linker else None,
                                      effective_linker=None, source_ownership=None, native_authority=None)
    origin = dict(registry=BASE.tree(args.registry_home), source_receipts=record["source_receipts"],
                  fixtures={name: BASE.BIND.inventory(Path(spec["root"]))
                            for name, spec in blueprint["fixtures"].items()})
    record["origin_inputs"] = BASE.write_json(args.output / "origin-inputs.json", origin)
    origin_descriptor = copy.deepcopy(record["origin_inputs"])
    validator = BASE.artifact(ROOT / "cache_transport_v2.py")
    args.bundle_witnesses = {}
    failure = None
    try:
        bundle = None
        for number, spec in ((1, initial), (2, successor)):
            bundle = physical_stage(args, blueprint, case, number, spec, initial, record, bundle)
    except (ValueError, OSError, KeyError) as error:
        failure = error
        record["primary_error"] = str(error)
    finally:
        guard_case(args, record, blueprint, tools, sdk, origin, origin_descriptor, validator)
    if failure is not None:
        raise failure
    BASE.require(all(item["status"] == "unchanged" for item in record["after_guards"]),
                 "negative source/tool/context after guard failed")
    record["status"] = "observed-local-negative-case"


def guard_case(args, record, blueprint, tools, sdk, origin, origin_descriptor, validator):
    comparisons = [("toolchain", lambda: BASE.tree(args.toolchain_root), tools),
                   ("sdk", lambda: INPUTS.context_inventory(args.sdk_root), sdk),
                   ("blueprint", lambda: BASE.artifact(args.blueprint), record["blueprint"])]
    for name, descriptor in record["tools"].items():
        comparisons.append(("tool-" + name, lambda name=name: BASE.artifact(getattr(args, name)), descriptor))
    if args.linker is not None:
        comparisons.append(("inspected-linker", lambda: BASE.artifact(args.linker),
                            record["observer_context"]["inspected_linker"]))
    comparisons.append(("observer-source", lambda: BASE.artifact(Path(blueprint["observer"]["path"])),
                        {key: blueprint["observer"][key] for key in ("path", "size", "sha256")}))
    comparisons.append(("origin-receipt", lambda: BASE.artifact(Path(origin_descriptor["path"])), origin_descriptor))
    comparisons.append(("transport-validator", lambda: BASE.artifact(ROOT / "cache_transport_v2.py"), validator))
    comparisons.append(("origin-registry", lambda: BASE.tree(args.registry_home), origin["registry"]))
    comparisons.append(("source-receipts", lambda: BASE.source_receipts(args), origin["source_receipts"]))
    for name, expected in origin["fixtures"].items():
        comparisons.append(("fixture-" + name, lambda name=name: BASE.BIND.inventory(
            Path(blueprint["fixtures"][name]["root"])), expected))
    record["after_guards"] = []
    for name, operation, expected in comparisons:
        try:
            observed = operation()
            witness = BASE.write_json(args.output / ("after-" + name + ".json"), observed)
            record["after_guards"].append(dict(name=name, witness=witness,
                                               status="unchanged" if observed == expected else "changed"))
        except (ValueError, OSError, KeyError) as error:
            record["after_guards"].append(dict(name=name, status="unavailable", error=str(error)))


def run_all(args, record):
    blueprint, descriptor = INPUTS.blueprint(args.blueprint, args.expected_blueprint_sha256)
    record["blueprint"] = descriptor
    cases = [case for case in blueprint["cases"] if case["id"] == args.case]
    BASE.require(len(cases) == 1, "negative case identity differs")
    case = cases[0]
    BASE.require(args.case not in ("T08", "T09-compiler"),
                 "T08 awaits actual resolved corpus; alternate compiler context remains unavailable")
    BASE.require(BASE.artifact(args.mbx)["sha256"] == blueprint["frozen_mbx"]["binary_sha256"],
                 "negative blueprint/MBX executable differs")
    initial = blueprint["fixtures"][case.get("baseline", case.get("fixture"))]
    successor = INPUTS.successor(blueprint, case)
    INPUTS.fixture(Path(initial["root"]), initial)
    if args.case != "T09-config":
        INPUTS.fixture(Path(successor["root"]), successor)
    execute_case(args, record, blueprint, case, initial, successor)


def arguments():
    INPUTS.closed_options(sys.argv[1:], ("--active-root", "--blueprint", "--sdk-root", "--linker",
                         "--case", "--expected-blueprint-sha256", "--expected-sdk-inventory-sha256",
                         "--expected-validator-sha256"))
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    for name in ("active-root", "blueprint", "sdk-root", "linker"):
        parser.add_argument("--" + name, type=Path, required=name != "linker")
    for name in ("case", "expected-blueprint-sha256", "expected-sdk-inventory-sha256", "expected-validator-sha256"):
        parser.add_argument("--" + name, required=True)
    extra, rest = parser.parse_known_args()
    original = sys.argv
    try:
        sys.argv = [original[0], *rest]
        args = BASE.arguments()
    finally:
        sys.argv = original
    for name, value in vars(extra).items():
        setattr(args, name, value)
    BASE.canonical(args.active_root.parent)
    BASE.require(not args.active_root.exists() and not args.active_root.is_symlink(), "new active root required")
    BASE.require(args.active_root.is_absolute() and not args.output.is_relative_to(args.active_root)
                 and not args.active_root.is_relative_to(args.output), "active and durable roots must be disjoint")
    BASE.canonical(args.sdk_root)
    if args.linker is not None:
        BASE.canonical(args.linker)
    BASE.canonical(args.blueprint)
    return args


def main():
    args = arguments()
    args.output.mkdir(mode=0o700)
    record = dict(schema=1, scope="local-negative-registry-case-v2", case=args.case, status="failed",
                  runs=[], states=[], native_authority=None, native_abi=None, hosted_t01_t03=None)
    lock = args.active_root.with_name(args.active_root.name + ".ownership")
    acquired = False
    try:
        BASE.require(BASE.artifact(ROOT / "cache_transport_v2.py")["sha256"] == args.expected_validator_sha256,
                     "reviewed V2 transport validator differs")
        lock.mkdir(mode=0o700)
        acquired = True
        record["exclusive_ownership"] = LIFECYCLE.allocation(lock)
        record["exclusive_ownership"]["nonce"] = uuid.uuid4().hex
        with V2.strict_execution(BASE, PROTOCOL):
            run_all(args, record)
        BASE.require(BASE.artifact(ROOT / "cache_transport_v2.py")["sha256"] == args.expected_validator_sha256,
                     "V2 transport validator changed")
    except (ValueError, OSError, KeyError) as error:
        record["error"] = str(error)
    finally:
        if acquired and not args.active_root.exists() and not args.active_root.is_symlink():
            lock.rmdir()
    record["artifacts"] = [BASE.artifact(path) for path in sorted(args.output.rglob("*"))
                           if path.is_file() and not path.is_symlink() and
                           ("logs" in str(path.parent) or "-reports" in str(path.parent))]
    BASE.write_json(args.output / "negative-execution.json", record)
    print(json.dumps(dict(status=record["status"], output=str(args.output), native_authority=None)))
    return 0 if record["status"] == "observed-local-negative-case" else 1


if __name__ == "__main__":
    sys.exit(main())

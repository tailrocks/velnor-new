#!/usr/bin/env python3
"""Exact locked itoa17→18 source transition; historical negative driver stays immutable."""
import argparse
import contextlib
import copy
import json
from pathlib import Path
import shutil
import sys
import tomllib
import importlib.util

ROOT = Path(__file__).resolve().parent

def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

NEG = load("t08_negative_driver", "run_negative_v2.py")
BASE = NEG.BASE
RESOLVE = load("t08_lock_parser", "resolve_t08.py")
ORIGINAL_ARGUMENTS = NEG.arguments
ORIGINAL_SEED = BASE.seed_registry
ORIGINAL_RECEIPT = NEG.input_receipt


def receipt17(args, blueprint):
    descriptor = blueprint["source_receipts"]["itoa17"]
    path = Path(descriptor["path"])
    observed = BASE.artifact(path)
    BASE.require(observed["sha256"] == args.expected_itoa17_receipt_sha256 and
                 all(observed[key] == descriptor[key] for key in ("path", "size", "sha256")),
                 "independent17 source receipt differs")
    receipt = json.loads(BASE.BIND.regular(path))
    expected = next(case for case in blueprint["cases"] if case["id"] == "T08")["expected17"]
    BASE.require({key: receipt["package"][key] for key in expected} == expected,
                 "reviewed17 package tuple differs")
    return receipt, observed


def verify_index(args, receipt, positive):
    root = args.registry_home / "registry/index" / args.source_relative.parent.name
    index, config = BASE.artifact(root / ".cache/it/oa/itoa"), BASE.artifact(root / "config.json")
    BASE.require(index["sha256"] == args.expected_registry_index_sha256 and
                 config["sha256"] == args.expected_index_config_sha256,
                 "independent dual registry index/config differs")
    raw = BASE.BIND.regular(Path(index["path"]))
    records = []
    for item in raw.split(b"\0"):
        if item.startswith(b"{"):
            records.append(json.loads(item))
    for version, checksum in (("1.0.17", receipt["package"]["checksum"]),
                              ("1.0.18", positive["registry"]["archive_sha256"])):
        selected = [r for r in records if r.get("name") == "itoa" and r.get("vers") == version]
        BASE.require(len(selected) == 1 and selected[0]["cksum"] == checksum
                     and selected[0].get("yanked") is False, "dual registry selected tuple differs")
        if version == "1.0.17":
            BASE.require(selected[0] == receipt["registry_index"]["selected_record"],
                         "reviewed17 index record differs")
    return dict(index=index, config=config)


def verify_sources(args, blueprint):
    receipt, descriptor = receipt17(args, blueprint)
    positive = json.loads(BASE.BIND.regular(ROOT / "manifest.json"))
    BASE.require(BASE.artifact(ROOT / "manifest.json")["sha256"] == args.expected_manifest_sha256,
                 "reviewed18 manifest differs")
    BASE.BIND.verify_registry(RESOLVE.registry_spec(receipt), args.registry17_archive, args.registry17_source)
    BASE.BIND.verify_registry(positive["registry"], args.registry_home / args.archive_relative,
                              args.registry_home / args.source_relative)
    BASE.require(args.registry17_source.parent == (args.registry_home / args.source_relative).parent,
                 "dual versions must share exact registry identity")
    return dict(itoa17=descriptor, itoa18=BASE.artifact(ROOT / "manifest.json"),
                index=verify_index(args, receipt, positive), native_authority=None), receipt, positive


def seed_registry(args, destination):
    ORIGINAL_SEED(args, destination)
    for source in (args.registry17_archive, args.registry17_source):
        relative = source.relative_to(args.registry_home)
        target = destination / relative
        BASE.require(not target.exists(), "dual seed17 target already exists")
        target.parent.mkdir(parents=True, exist_ok=True)
        if source.is_dir():
            shutil.copytree(source, target)
        else:
            shutil.copyfile(source, target)


def expected_package(args, run):
    version = "1.0.17" if run["id"] == 1 else "1.0.18"
    checksum = args.dual_receipt17["package"]["checksum"] if run["id"] == 1 else args.dual_positive["registry"]["archive_sha256"]
    return dict(name="itoa", version=version, source="registry+https://github.com/rust-lang/crates.io-index", checksum=checksum)


def public_dependency(stdout, package, owned_root):
    identities = []
    with stdout.open("rb") as stream:
        while line := stream.readline(8 * 1024 * 1024 + 1):
            BASE.require(len(line) <= 8 * 1024 * 1024, "public Cargo line exceeds bound")
            if not line.startswith(b"{"):
                continue
            value = json.loads(line)
            if value.get("reason") == "compiler-artifact" and value.get("target", {}).get("name") == "itoa":
                identity = value["package_id"]
                BASE.require(identity == package["source"] + "#itoa@" + package["version"],
                             "current public dependency identity differs")
                artifacts = []
                for filename in value["filenames"]:
                    path = BASE.canonical(Path(filename))
                    BASE.require(path.is_relative_to(owned_root), "public dependency artifact escapes state")
                    artifacts.append(BASE.artifact(path))
                BASE.require(artifacts, "current dependency artifacts missing")
                identities.append(dict(package_id=identity, target=value["target"], artifacts=artifacts, fresh=value.get("fresh")))
    BASE.require(len(identities) == 1, "exactly one current public itoa artifact required")
    return identities[0]


def input_receipt(args, run, spec, registry, name):
    original = ORIGINAL_RECEIPT(args, run, spec, registry, name)
    workspace = Path(run["cwd"])
    package = expected_package(args, run)
    RESOLVE.generated_lock(workspace / "Cargo.lock", package)
    cargo_home = workspace.parent / "cargo-home"
    BASE.BIND.verify_registry(RESOLVE.registry_spec(args.dual_receipt17),
        cargo_home / args.registry17_archive.relative_to(args.registry_home),
        cargo_home / args.registry17_source.relative_to(args.registry_home))
    current = None
    if name == "after" and run["commands"][-1]["returncode"] == 0:
        current = public_dependency(Path(run["commands"][-1]["stdout"]["path"]), package, workspace.parent)
    value = dict(original_inputs=original, expected_package=package,
                 current_dependency_artifact=current, lock=BASE.artifact(workspace / "Cargo.lock"),
                 dual_source_receipts=args.dual_receipts, native_authority=None)
    return BASE.write_json(workspace.parent / "logs" / (name + "-dual-inputs.json"), value)


def sealed_blueprint(args):
    blueprint, descriptor = NEG.INPUTS.blueprint(args.blueprint, args.expected_blueprint_sha256)
    seal_path = args.blueprint.parent / "seal.json"
    seal_descriptor = BASE.artifact(seal_path)
    BASE.require(seal_descriptor["sha256"] == args.expected_seal_sha256, "independent source seal differs")
    seal = json.loads(BASE.BIND.regular(seal_path))
    BASE.require(seal["blueprint"] == descriptor and seal["audit"]["sha256"] == args.expected_audit_sha256,
                 "source seal blueprint/audit differs")
    audit = seal["audit"]
    BASE.require(BASE.artifact(Path(audit["path"])) == audit, "independent bootstrap audit bytes differ")
    for row in seal["copy_only"]:
        actual = BASE.artifact(Path(row["destination"]))
        BASE.require(actual == dict(path=row["destination"],size=row["size"],sha256=row["sha256"]),
                     "sealed corpus source differs")
    return blueprint, descriptor, seal_descriptor


def run_all(args, record):
    BASE.require(args.case == "T08", "this successor only executes T08")
    blueprint, descriptor, seal = sealed_blueprint(args)
    record.update(blueprint=descriptor, source_seal=seal)
    args.dual_receipts, args.dual_receipt17, args.dual_positive = verify_sources(args, blueprint)
    source_before = copy.deepcopy(args.dual_receipts)
    record["dual_source_receipts"] = source_before
    case = next(case for case in blueprint["cases"] if case["id"] == "T08")
    BASE.require(BASE.artifact(args.mbx)["sha256"] == blueprint["frozen_mbx"]["binary_sha256"],
                 "source blueprint/MBX differs")
    initial, successor = (blueprint["fixtures"][case[key]] for key in ("baseline", "successor"))
    for spec in (initial, successor):
        NEG.INPUTS.fixture(Path(spec["root"]), spec)
    try:
        NEG.execute_case(args, record, blueprint, case, initial, successor)
    finally:
        record["status"] = "failed"
        record["source_seal_after_guard"] = dict(status="unavailable")
        try:
            _, after_blueprint, after_seal = sealed_blueprint(args)
            record["source_seal_after_guard"] = dict(status="unchanged" if after_blueprint == descriptor and after_seal == seal else "changed")
        except (ValueError,OSError,KeyError) as error:
            record["source_seal_after_guard"]["error"] = str(error)
        record["dual_source_after_guard"] = dict(status="unavailable")
        try:
            observed, _, _ = verify_sources(args, blueprint)
            record["dual_source_after_guard"] = dict(status="unchanged" if observed == source_before else "changed", observed=observed)
        except (ValueError,OSError,KeyError) as error:
            record["dual_source_after_guard"]["error"] = str(error)
    BASE.require(record["dual_source_after_guard"]["status"] == "unchanged" and
                 record["source_seal_after_guard"]["status"] == "unchanged", "dual source/seal after guard failed")
    record["status"] = "observed-local-negative-case"


def arguments():
    options = ("--registry17-archive", "--registry17-source", "--expected-itoa17-receipt-sha256",
               "--expected-registry-index-sha256", "--expected-index-config-sha256",
               "--expected-seal-sha256", "--expected-audit-sha256")
    NEG.INPUTS.closed_options(sys.argv[1:], options + ("--active-root", "--blueprint", "--sdk-root", "--linker", "--case", "--expected-blueprint-sha256", "--expected-sdk-inventory-sha256", "--expected-validator-sha256"))
    parser = argparse.ArgumentParser(add_help=False,allow_abbrev=False)
    for option in options:
        parser.add_argument(option, required=True, type=Path if option.startswith("--registry17") else str)
    extra, rest = parser.parse_known_args()
    original = sys.argv
    try:
        sys.argv = [original[0], *rest]
        args = ORIGINAL_ARGUMENTS()
    finally:
        sys.argv = original
    for name,value in vars(extra).items():
        setattr(args,name,value)
    for name in ("registry17_archive", "registry17_source"):
        path = BASE.canonical(getattr(args,name))
        BASE.require(path.is_relative_to(args.registry_home), "private dual seed path required")
    return args


@contextlib.contextmanager
def hooks():
    original = NEG.arguments, NEG.run_all, NEG.input_receipt, BASE.seed_registry
    NEG.arguments, NEG.run_all, NEG.input_receipt, BASE.seed_registry = arguments, run_all, input_receipt, seed_registry
    try:
        yield
    finally:
        NEG.arguments, NEG.run_all, NEG.input_receipt, BASE.seed_registry = original


def main():
    with hooks():
        return NEG.main()


if __name__ == "__main__":
    sys.exit(main())

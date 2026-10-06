#!/usr/bin/env python3
"""Qualify one exact native owned Actionlint binary against real CLI fixtures."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import tempfile
import time

VERSION = "1.7.12-velnor-cache-mode.1+patch.7c81196d7996"
HOSTS = {
    ("Darwin", "arm64"): ("darwin", "arm64", "aarch64-apple-darwin"),
    ("Darwin", "x86_64"): ("darwin", "amd64", "x86_64-apple-darwin"),
    ("Linux", "x86_64"): ("linux", "amd64", "x86_64-unknown-linux-gnu"),
}
ROOT = Path(__file__).resolve().parent
MANIFEST_SHA256 = "1381f8b4574817bcbb591ddb49b376b0b0d64a8c0f6eaa6ecdb528f77eb05fe6"
FIXTURES_SHA256 = "ee861042b4fd7532a2002f56d392d7987123af5008f931aed396af805371e647"
RECIPE = ROOT.parents[1] / "actionlint-owned-build-recipe.json"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def pairs(items):
    result = {}
    for key, value in items:
        require(key not in result, "duplicate JSON key: " + key)
        result[key] = value
    return result


def closed(value, keys):
    require(isinstance(value, dict) and set(value) == set(keys.split()),
            "receipt fields differ from closed identity contract")


def read_json(path):
    require(path.is_file() and not path.is_symlink(), "regular JSON file required")
    raw = path.read_bytes()
    return json.loads(raw, object_pairs_hook=pairs), sha(raw)


def binary_host(data):
    if len(data) >= 64 and data[:7] == b"\x7fELF\x02\x01\x01":
        require(int.from_bytes(data[16:18], "little") in (2, 3), "ELF executable required")
        return {62: ("linux", "amd64"), 183: ("linux", "arm64")}.get(
            int.from_bytes(data[18:20], "little"))
    if len(data) >= 32 and data[:4] == b"\xcf\xfa\xed\xfe":
        require(int.from_bytes(data[12:16], "little") == 2, "MachO executable required")
        return {0x0100000C: ("darwin", "arm64"), 0x01000007: ("darwin", "amd64")}.get(
            int.from_bytes(data[4:8], "little"))
    raise ValueError("unsupported executable format or architecture")


def native_hardware(host):
    if host[0] != "darwin":
        return {}
    evidence = {}
    for name in ("sysctl.proc_translated", "hw.optional.arm64"):
        proc = subprocess.run(["/usr/sbin/sysctl", "-n", name], capture_output=True,
                              text=True, timeout=10, check=False)
        evidence[name] = dict(returncode=proc.returncode, stdout=proc.stdout,
                              stderr=proc.stderr)
    translated = evidence["sysctl.proc_translated"]
    hardware = evidence["hw.optional.arm64"]
    require(translated["returncode"] in (0, 1) and (translated["returncode"] != 0
            or translated["stdout"].strip() == "0"), "translated execution is not native proof")
    require(hardware["returncode"] == 0 and hardware["stdout"].strip() in ("0", "1"),
            "cannot prove native Mac hardware architecture")
    require(hardware["stdout"].strip() == ("1" if host[1] == "arm64" else "0"),
            "process architecture differs from native Mac hardware")
    return evidence


def identity(args):
    binary = args.actionlint
    require(binary.is_absolute() and binary.is_file() and not binary.is_symlink(),
            "absolute regular executable path required")
    require(os.access(binary, os.X_OK), "binary must be executable")
    require(args.expected_version == VERSION, "exact qualified owned version required")
    host = HOSTS.get((platform.system(), platform.machine()))
    require(host is not None, "unsupported native host")
    hardware = native_hardware(host)
    data = binary.read_bytes()
    require(binary_host(data) == host[:2], "binary architecture differs from host")
    receipt, receipt_sha = read_json(args.build_receipt)
    closed(receipt, "schema status tool version version_banner target source compiler "
           "compiler_binary_sha256 compiler_asset native_execution recipe_sha256 artifact")
    closed(receipt["compiler"], "go_version goos goarch")
    closed(receipt["artifact"], "binary_sha256")
    closed(receipt["native_execution"], "platform_system platform_machine hardware_native")
    require(receipt["native_execution"] == dict(platform_system=platform.system(),
            platform_machine=platform.machine(), hardware_native=True), "native execution receipt mismatch")
    require(type(receipt["schema"]) is int and receipt["schema"] == 1 and receipt["status"] == "native-build-candidate"
            and receipt["tool"] == "actionlint", "invalid candidate receipt")
    recipe, recipe_sha = read_json(RECIPE)
    require(receipt["recipe_sha256"] == recipe_sha, "build recipe digest mismatch")
    require(receipt["source"] == recipe["source"], "qualified source identity mismatch")
    approved = recipe["compiler"]["approved_assets"].get(host[2])
    require(approved is not None and receipt["compiler_asset"] == approved,
            "approved native compiler asset receipt missing or mismatched")
    closed(approved, "asset_url archive_sha256 compiler_binary_sha256 toolchain_tree_sha256 go_version")
    for key in ("archive_sha256", "compiler_binary_sha256", "toolchain_tree_sha256"):
        require(isinstance(approved[key], str) and re.fullmatch(r"[a-f0-9]{64}", approved[key])
                and approved[key] != "0" * 64, "invalid approved compiler digest")
    require(receipt["compiler_binary_sha256"] == approved["compiler_binary_sha256"]
            and approved["go_version"] == "go1.27.1", "compiler executable identity mismatch")
    require(recipe["reported_version"] == args.expected_version, "recipe version mismatch")
    require(recipe["targets"][host[2]] == list(host[:2]), "recipe native host mismatch")
    expected_banner = recipe["version_banner_template"].format(goos=host[0], goarch=host[1])
    require(receipt["version_banner"] == expected_banner, "qualified version banner mismatch")
    require(receipt["version"] == args.expected_version, "receipt version mismatch")
    require(receipt["artifact"]["binary_sha256"] == sha(data), "receipt binary digest mismatch")
    compiler = receipt["compiler"]
    require(compiler["go_version"] == "go1.27.1", "unqualified Go compiler version")
    require((compiler["goos"], compiler["goarch"]) == host[:2], "receipt compiler host mismatch")
    require(receipt["target"] == host[2], "receipt target mismatch")
    return binary, sha(data), receipt, receipt_sha, host, hardware


def fixtures():
    cases, manifest_sha = read_json(ROOT / "cases.json")
    require(manifest_sha == MANIFEST_SHA256 and len(cases) == 47, "qualified case inventory mismatch")
    records = []
    names = set()
    for case in cases:
        require(case["case"] not in names, "duplicate case")
        names.add(case["case"])
        file = ROOT / case["fixture"]
        require(file.parent == ROOT / "fixtures" and file.suffix == ".yml"
                and file.is_file() and not file.is_symlink(), "invalid fixture path")
        require(type(case["accept"]) is bool, "case accept must be boolean")
        require(all(isinstance(value, str) for value in case["diagnostics"]),
                "diagnostics must contain regex strings")
        data = file.read_bytes()
        records.append((case, data, sha(data)))
    listed = {case["fixture"] for case, _, _ in records}
    actual = {"fixtures/" + file.name for file in (ROOT / "fixtures").iterdir()}
    require(listed == actual, "fixture inventory differs from manifest")
    aggregate = "".join(case["fixture"] + "\0" + digest + "\n"
                        for case, _, digest in sorted(records, key=lambda value: value[0]["fixture"]))
    fixture_sha = sha(aggregate.encode())
    require(fixture_sha == FIXTURES_SHA256, "qualified fixture bytes mismatch")
    return records, manifest_sha, fixture_sha


def invoke(binary, argv, directory):
    env = {key: os.environ[key] for key in ("PATH", "SYSTEMROOT", "LANG", "LC_ALL")
           if key in os.environ}
    env.update(HOME=str(directory), XDG_CONFIG_HOME=str(directory), NO_COLOR="1")
    started = time.monotonic_ns()
    try:
        proc = subprocess.run([str(binary), *argv], cwd=directory, env=env,
                              capture_output=True, text=True, timeout=30, check=False)
        return dict(argv=argv, returncode=proc.returncode, stdout=proc.stdout,
                    stderr=proc.stderr, duration_ns=time.monotonic_ns() - started)
    except (OSError, subprocess.TimeoutExpired) as error:
        return dict(argv=argv, returncode=None, stdout="", stderr="",
                    duration_ns=time.monotonic_ns() - started, error=str(error))


def run_case(binary, case, data, digest, directory):
    destination = directory / (case["case"] + ".yml")
    destination.write_bytes(data)
    # These standard flags disable optional external language analyzers only.
    # Actionlint's native syntax, expression, action and workflow checks stay active.
    measured = invoke(binary, ["-no-color", "-shellcheck=", "-pyflakes=", str(destination)], directory)
    combined = measured["stdout"] + measured["stderr"]
    if case["accept"]:
        passed = measured["returncode"] == 0 and not combined.strip()
    else:
        passed = measured["returncode"] == 1 and all(
            re.search(pattern, combined) for pattern in case["diagnostics"])
    return dict(case=case["case"], fixture=case["fixture"], fixture_sha256=digest,
                expected_accept=case["accept"], expected_diagnostics=case["diagnostics"],
                passed=bool(passed), **measured)


def qualify(args):
    binary, binary_sha, receipt, receipt_sha, host, hardware = identity(args)
    records, manifest_sha, fixture_sha = fixtures()
    with tempfile.TemporaryDirectory(prefix="owned-actionlint-qualification-") as temporary:
        directory = Path(temporary)
        version = invoke(binary, ["-version"], directory)
        banner = version["stdout"].strip()
        require(version["returncode"] == 0 and not version["stderr"], "version CLI failed")
        require(banner == receipt["version_banner"].strip(), "measured version banner differs from receipt")
        require(banner.splitlines()[0] == args.expected_version
                and banner.splitlines()[1] == "velnor-owned-cache-mode",
                "binary does not report exact owned version and installation origin")
        results = [run_case(binary, case, data, digest, directory)
                   for case, data, digest in records]
    unchanged = sha(binary.read_bytes()) == binary_sha
    passed = unchanged and all(result["passed"] for result in results)
    return dict(schema=1, tool="actionlint", version=args.expected_version,
                status="PASS" if passed else "FAIL", binary=str(binary),
                binary_sha256=binary_sha, binary_unchanged=unchanged,
                build_receipt_sha256=receipt_sha, host=dict(goos=host[0], goarch=host[1], target=host[2]),
                native_hardware=hardware, version_measurement=version, cases_manifest_sha256=manifest_sha,
                fixtures_sha256=fixture_sha, runner_sha256=sha(Path(__file__).read_bytes()),
                results=results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--actionlint", type=Path, required=True)
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--build-receipt", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    require(not args.output.exists() and not args.output.is_symlink(), "output already exists")
    try:
        report = qualify(args)
    except (ValueError, KeyError, TypeError, OSError, json.JSONDecodeError,
            subprocess.TimeoutExpired) as error:
        report = dict(schema=1, tool="actionlint", status="FAIL", error=str(error))
    with args.output.open("x", encoding="utf-8") as destination:
        json.dump(report, destination, indent=2, sort_keys=True)
        destination.write("\n")
    results = report.get("results", [])
    print(json.dumps(dict(output=str(args.output), status=report["status"],
                         passed=sum(result["passed"] for result in results), total=len(results),
                         failed=[result["case"] for result in results if not result["passed"]],
                         error=report.get("error"))))
    raise SystemExit(0 if report["status"] == "PASS" else 1)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Pure, bounded fixture evidence join; JSON flags never mint native authority."""

import argparse
from dataclasses import asdict, dataclass
import importlib.util
import json
from pathlib import Path
import re
from typing import Any

ROOT = Path(__file__).resolve().parent
SUPPORT_SPEC = importlib.util.spec_from_file_location("join_support", ROOT / "join_support.py")
SUPPORT = importlib.util.module_from_spec(SUPPORT_SPEC)
SUPPORT_SPEC.loader.exec_module(SUPPORT)

SCOPE = SUPPORT.SCOPE
MAX_BYTES = SUPPORT.MAX_BYTES
MAX_REPORT_BYTES = SUPPORT.MAX_REPORT_BYTES
MAX_INVENTORY_BYTES = SUPPORT.MAX_INVENTORY_BYTES
MAX_ARTIFACTS = SUPPORT.MAX_ARTIFACTS
SHA = SUPPORT.SHA
UUID = SUPPORT.UUID
require = SUPPORT.require
keys = SUPPORT.keys
sha = SUPPORT.sha
regular = SUPPORT.regular
artifact = SUPPORT.artifact
object_pairs = SUPPORT.object_pairs
document = SUPPORT.document

@dataclass(frozen=True)
class LifetimeEvidence:
    scope: str
    status: str
    compiler_scope: str
    observed_fixture_scope: str
    native_process_scope: str
    generic_taskwide_status: str
    reasons: tuple[str, ...]

@dataclass(frozen=True)
class JoinedEvidence:
    schema: int
    scope: str
    status: str
    execution_record_sha256: str
    manifest_sha256: str
    native_authority: None
    lifetime: LifetimeEvidence
    observed_reports: tuple[dict[str, Any], ...]
    source_receipts: dict[str, Any]
def identity(value: Any, correlation: str, role: str | None = None) -> None:
    fields = "session_id root_session_id parent_session_id caller_correlation"
    if role is not None:
        fields += " command_role"
    keys(value, fields, "native identity")
    require(all(type(value[key]) is str and UUID.fullmatch(value[key])
                for key in ("session_id", "root_session_id")), "invalid session UUID")
    parent = value["parent_session_id"]
    require(parent is None and value["session_id"] == value["root_session_id"],
            "fixture must own independent root session")
    require(value["caller_correlation"] == correlation, "run correlation differs")
    if role is not None:
        require(value["command_role"] == role, "native command role differs")

def native_reports(command: dict[str, Any], correlation: str,
                   sessions: set[str]) -> list[dict[str, Any]]:
    reports, receipts = [], []
    artifacts = command["report_artifacts"]
    require(type(artifacts) is list and len(artifacts) <= MAX_ARTIFACTS,
            "invalid native artifacts")
    for descriptor in artifacts:
        raw = artifact(descriptor, MAX_REPORT_BYTES)
        parsed = document(raw)
        require(type(parsed) is dict and type(parsed.get("schema_version")) is int
                and parsed["schema_version"] == 1, "unsupported native schema")
        if "completed" in parsed:
            keys(parsed, "schema_version completed mbx_version source_base_version "
                 "identity workload statistics", "completed report")
            require(parsed["completed"] is True, "incomplete native report")
            reports.append((descriptor, parsed))
        else:
            require(len(raw) <= 16 * 1024 and type(parsed.get("event_id")) is str and
                    re.fullmatch("[0-9a-f]{32}", parsed["event_id"]), "native receipt bound/token differs")
            keys(parsed, "schema_version mbx_version source_base_version identity "
                 "event_id adapter event_kind delivery", "native receipt")
            receipts.append((descriptor, parsed))
    require(len(reports) <= 1, "multiple owner reports for command")
    if not reports:
        require(not receipts, "native receipts without owner report")
        return []
    descriptor, report = reports[0]
    role = "cargo_" + command["argv"][1]
    identity(report["identity"], correlation, role)
    session = report["identity"]["session_id"]
    admissions = command.get("admission_artifacts", [])
    require(type(admissions) is list and len(admissions) <= MAX_ARTIFACTS, "invalid admission artifacts")
    for item in admissions:
        artifact(item)
        relative = Path(item["path"]).relative_to(Path(descriptor["path"]).parent)
        require(len(relative.parts) >= 2 and relative.parts[0] == ".mbx-admissions-" + session,
                "opaque admission artifact owner differs")
    require(session not in sessions, "replayed native session")
    sessions.add(session)
    require(Path(descriptor["path"]).name == session + ".json", "report filename differs")
    keys(report["workload"], "outcome exit_code", "workload")
    code = report["workload"]["exit_code"]
    require((type(code) is int and code == command["returncode"]) or
            (code is None and command["returncode"] < 0), "native command exit code differs")
    for _, receipt in receipts:
        identity(receipt["identity"], correlation)
        expected = {key: value for key, value in report["identity"].items()
                    if key != "command_role"}
        require(receipt["identity"] == expected, "receipt owning session differs")
        require(all(receipt[key] == report[key]
                    for key in ("mbx_version", "source_base_version")),
                "receipt native version differs")
    require(type(report["statistics"]) is dict
            and type(report["statistics"].get("measurement")) is dict,
            "native observed measurement unavailable")
    # Preserve the original statistics, including null/Unknown and local flags.
    return [dict(artifact=descriptor, identity=report["identity"],
                 workload=report["workload"], statistics=report["statistics"],
                 receipts=[dict(artifact=item, receipt=value) for item, value in receipts],
                 opaque_admission_artifacts=admissions)]

def input_snapshot(descriptor: Any, manifest: dict[str, Any], digest: str,
                   cwd: str, registry_inputs: dict[str, Any]) -> None:
    value = document(artifact(descriptor, MAX_REPORT_BYTES))
    keys(value, "status manifest_sha256 fixture_root fixture_inventory_sha256 "
         "registry_archive_sha256 registry_inventory_sha256 registry_inputs native_authority", "input snapshot")
    require(value == dict(status="fixture-inputs-verified", manifest_sha256=digest,
                         fixture_root=cwd,
                         fixture_inventory_sha256=manifest["fixture"]["inventory_sha256"],
                         registry_archive_sha256=manifest["registry"]["archive_sha256"],
                         registry_inventory_sha256=manifest["registry"]["inventory_sha256"],
                         registry_inputs=registry_inputs, native_authority=None), "input snapshot binding differs")

def configurations(value: Any, cwd: str, env: dict[str, str]) -> None:
    require(type(value) is list, "configuration evidence required")
    workspace, home, cargo = Path(cwd), Path(env["HOME"]), Path(env["CARGO_HOME"])
    expected = set()
    for ancestor in (workspace, *workspace.parents):
        expected.update(str(ancestor / name) for name in
                        (".cargo/config", ".cargo/config.toml", ".mbx.toml"))
    expected.update(str(path) for path in (cargo / "config", cargo / "config.toml",
                    home / ".config/mbx/config.toml",
                    home / "Library/Application Support/mbx/config.toml"))
    require(len(value) == len(expected), "configuration discovery count differs")
    actual = set()
    for item in value:
        keys(item, "path status", "configuration input")
        require(item["status"] == "absent", "unreviewed configuration input")
        require(type(item["path"]) is str, "configuration path required")
        path = Path(item["path"])
        require(not path.exists() and not path.is_symlink(), "configuration absence differs")
        actual.add(str(path))
    require(actual == expected, "configuration discovery inputs differ")

def controlled_environment(env: Any, tools: dict[str, Any]) -> None:
    allowed = "PATH HOME CARGO_HOME RUSTUP_HOME RUSTC CARGO CARGO_TARGET_DIR " + \
        "CARGO_NET_OFFLINE CARGO_BUILD_JOBS XDG_CONFIG_HOME XDG_CACHE_HOME " + \
        "XDG_DATA_HOME TMPDIR LANG LC_ALL TZ MBX_CACHE_DIR MBX_GC_AUTO " + \
        "MBX_CACHE_EXPORT_GROUP MBX_SUMMARY MBX_DISPLAY"
    keys(env, allowed, "controlled environment")
    require(all(type(value) is str and "\x00" not in value for value in env.values()),
            "environment string required")
    require(env["RUSTC"] == tools["rustc"]["path"] and
            env["CARGO"] == tools["cargo"]["path"], "compiler route differs")
    require(env["CARGO_NET_OFFLINE"] == "true" and env["CARGO_BUILD_JOBS"] == "2",
            "offline fixture with two compiler jobs required")

def command_evidence(command: Any, run: dict[str, Any], expected: list[str],
                     tools: dict[str, Any], manifest: dict[str, Any], digest: str,
                     correlation: str, sessions: set[str]) -> list[dict[str, Any]]:
    keys(command, "argv cwd environment returncode stdout stderr fixture_before "
         "fixture_after config_inputs_before config_inputs_after report_artifacts admission_artifacts", "fixture command")
    require(command["argv"] == [tools["mbx"]["path"], *expected[1:]], "fixture argv differs")
    require(command["cwd"] == run["cwd"], "fixture working directory differs")
    require(type(command["returncode"]) is int, "command exit code required")
    env = command["environment"]
    keys(env, " ".join(run["environment"]) +
         " MBX_STATS_REPORT_DIR MBX_REPORT_CORRELATION_ID", "command environment")
    require(all(env[key] == value for key, value in run["environment"].items()),
            "command controlled environment differs")
    require(env["MBX_REPORT_CORRELATION_ID"] == correlation, "command correlation differs")
    configurations(command["config_inputs_before"], run["cwd"], env)
    configurations(command["config_inputs_after"], run["cwd"], env)
    for stage in ("fixture_before", "fixture_after"):
        input_snapshot(command[stage], manifest, digest, run["cwd"], run["registry_inputs"])
    artifact(command["stdout"])
    artifact(command["stderr"])
    require(all(Path(item["path"]).parent == Path(env["MBX_STATS_REPORT_DIR"])
                for item in command["report_artifacts"]), "native artifact report directory differs")
    return native_reports(command, correlation, sessions)
def inventory(descriptor: Any, root: Path) -> None:
    records = document(artifact(descriptor, MAX_INVENTORY_BYTES))
    inventory_bytes(records, root)
def inventory_bytes(records: Any, root: Path, fixture_lock_alias: bool = False) -> None:
    require(type(records) is list and len(records) <= MAX_ARTIFACTS, "inventory bound/schema differs")
    if fixture_lock_alias:
        expected_locks = [item["path"] for item in records if item.get("path") in
                          ("Cargo.lock", "Cargo.lock.fixture")]
        require(expected_locks == ["Cargo.lock.fixture"], "pinned fixture lock name differs")
    actual = []
    for path in sorted(root.rglob("*")):
        require(not path.is_symlink(), "inventory symlink forbidden")
        if path.is_dir():
            continue
        require(path.is_file(), "inventory special file forbidden")
        actual.append(dict(path=path.relative_to(root).as_posix(), size=path.stat().st_size,
                           sha256=sha(regular(path))))
    if fixture_lock_alias:
        lock_names = [item["path"] for item in actual if item["path"] in
                      ("Cargo.lock", "Cargo.lock.fixture")]
        require(len(lock_names) == 1, "fixture must contain one pinned Cargo lock")
        if lock_names == ["Cargo.lock"]:
            actual = [dict(item, path="Cargo.lock.fixture") if item["path"] == "Cargo.lock"
                      else item for item in actual]
    require(records == actual, "actual consumed/distribution inventory differs")

def auxiliary(commands: Any, run: dict[str, Any], record: dict[str, Any],
              manifest: dict[str, Any], probes: bool = False) -> None:
    require(type(commands) is list and len(commands) <= 4, "auxiliary commands bound")
    probe_args = (("mbx", ["--version"]), ("cargo", ["--version", "--verbose"]),
                  ("rustc", ["--version", "--verbose"]), ("rustc", ["--print", "sysroot"]))
    for index, command in enumerate(commands):
        base = "argv cwd environment returncode stdout stderr"
        extras = set(command) - set(base.split())
        require(extras <= {"comparison_state", "export_observed", "bundle_inventory",
                           "input_bundle_inventory", "retained_bundle", "consumed_copy_exists_after"},
                "auxiliary schema differs")
        keys(command, base + " " + " ".join(extras), "auxiliary command")
        require(command["cwd"] == run["cwd"] and command["environment"] == run["environment"],
                "auxiliary execution binding differs")
        argv = command["argv"]
        if probes:
            name, extra = probe_args[index]
            require(argv == [record["tools"][name]["path"], *extra], "tool probe argv differs")
        else:
            require(type(argv) is list and len(argv) >= 4, "transport argv differs")
            root = Path(run["cwd"]).parent
            baseline = str(root / "comparison-state.json")
            routes = {"comparison-state": [baseline, "--json"],
                      "import": [str(root / "import-bundle"), "--comparison-state", baseline,
                                 "--json", "--", *manifest["commands"][0][1:]],
                      "export": [str(root / "mbx-cache-bundle"), "--format", "directory", "--json",
                                 "--group", record["run_attempt_id"], "--compare", baseline]}
            require(argv == [record["tools"]["mbx"]["path"], "cache", argv[2],
                             *routes.get(argv[2], [])], "transport argv differs")
        require(type(command["returncode"]) is int, "auxiliary exit code required")
        artifact(command["stdout"])
        artifact(command["stderr"])
        for key in ("comparison_state", "bundle_inventory", "input_bundle_inventory"):
            if key in command:
                artifact(command[key])
        if "retained_bundle" in command:
            inventory(command["input_bundle_inventory"], Path(command["retained_bundle"]))
        if "export_observed" in command:
            require(document(artifact(command["stdout"])) == command["export_observed"],
                    "export observation differs from stdout")

def retained_sources(receipts: Any, expected: dict[str, str]) -> dict[str, Any]:
    keys(receipts, "mbx compiler", "source receipts")
    for name, descriptor in receipts.items():
        if descriptor is None:
            require(name not in expected, "expected source receipt missing")
            continue
        require(name in expected and descriptor["sha256"] == expected[name],
                "source receipt lacks independent digest")
        value = document(artifact(descriptor, MAX_REPORT_BYTES))
        require(type(value) is dict, "source receipt object required")
        for record in value.get("source_records", []):
            require(type(record) is dict and "artifact" in record,
                    "source inventory artifact missing")
            artifact(record["artifact"])
    return receipts

def join(record_descriptor: dict[str, Any], manifest_descriptor: dict[str, Any],
         expected_sources: dict[str, str] | None = None) -> JoinedEvidence:
    record = document(artifact(record_descriptor, MAX_REPORT_BYTES))
    manifest_raw = artifact(manifest_descriptor, MAX_REPORT_BYTES)
    manifest, digest = document(manifest_raw), sha(manifest_raw)
    fields = "schema scope execution_kind run_attempt_id status runs native_authority " + \
        "native_abi hosted_t01_t03 host limitations manifest_sha256 source_receipts " + \
        "fixture_inventory_sha256 registry_archive_sha256 registry_inventory_sha256 " + \
        "toolchain_inventory tools registry_seed_inventory tool_observations artifacts"
    keys(record, fields + (" error" if "error" in record else ""), "execution record")
    require(type(record["schema"]) is int and record["schema"] == 1
            and record["scope"] == manifest["scope"] == SCOPE, "unsupported fixture schema/scope")
    require(record["native_authority"] is None and record["native_abi"] is None
            and record["hosted_t01_t03"] is None, "replayed native authority flag")
    require(record["manifest_sha256"] == digest, "independently reviewed manifest differs")
    require(record["execution_kind"] == "local" and record["status"] in ("observed", "failed"),
            "unsupported execution kind/status")
    for key, expected in (("fixture_inventory_sha256", manifest["fixture"]["inventory_sha256"]),
                          ("registry_archive_sha256", manifest["registry"]["archive_sha256"]),
                          ("registry_inventory_sha256", manifest["registry"]["inventory_sha256"])):
        require(record[key] == expected, "execution inventory binding differs")
    keys(record["tools"], "mbx cargo rustc", "tool distribution")
    for descriptor in record["tools"].values():
        artifact(descriptor)
    closure = manifest["source_review"]
    require(manifest["commands"] == [["cargo", op, "--locked", "--offline", "--lib",
            "--message-format=json-render-diagnostics"] for op in ("build", "check")] and
            closure["fixture_build_script"] is False and closure["dependency_build_script"] is False and
            closure["enabled_proc_macros"] == [] and closure["enabled_registry_dependencies"] == [] and
            closure["compiled_dependency_sources"] == ["src/lib.rs", "src/u128_ext.rs"],
            "reviewed Rustc library source closure differs")
    sources = retained_sources(record["source_receipts"], expected_sources or {})
    for name in ("toolchain_inventory", "registry_seed_inventory"):
        artifact(record[name])
    keys(record["host"], "system release machine", "native host")
    require(all(type(value) is str and value for value in record["host"].values()),
            "native host identity required")
    require(type(record["run_attempt_id"]) is str and record["run_attempt_id"],
            "run attempt identity required")
    inventory(record["toolchain_inventory"], Path(record["tools"]["rustc"]["path"]).parent.parent)
    reports, sessions, run_ids = [], set(), set()
    require(type(record["runs"]) is list and 1 <= len(record["runs"]) <= 3, "invalid run count")
    for run in record["runs"]:
        keys(run, "id cwd environment config_inputs registry_archive registry_source registry_inputs commands transport",
             "run")
        require(type(run["id"]) is int and 1 <= run["id"] <= 3 and run["id"] not in run_ids,
                "replayed run identity")
        run_ids.add(run["id"])
        controlled_environment(run["environment"], record["tools"])
        inventory_bytes(manifest["fixture"]["files"], Path(run["cwd"]), fixture_lock_alias=True)
        configurations(run["config_inputs"], run["cwd"], run["environment"])
        inventory(run["registry_inputs"], Path(run["cwd"]).parent / "cargo-home/registry")
        require(sha(regular(Path(run["registry_archive"]))) == manifest["registry"]["archive_sha256"],
                "registry archive origin differs")
        registry_files = sorted(manifest["registry"]["files"] +
                                [manifest["registry"]["local_extraction_marker"]], key=lambda item: item["path"])
        inventory_bytes(registry_files, Path(run["registry_source"]))
        auxiliary(run["transport"], run, record, manifest)
        require(type(run["commands"]) is list and len(run["commands"]) <= 2,
                "invalid fixture commands")
        for index, command in enumerate(run["commands"]):
            correlation = record["run_attempt_id"] + ":" + str(run["id"]) + ":command-" + str(index + 1)
            reports.extend(command_evidence(command, run, manifest["commands"][index],
                           record["tools"], manifest, digest, correlation, sessions))
    auxiliary(record["tool_observations"], record["runs"][0], record, manifest, True)
    for descriptor in record["artifacts"]:
        artifact(descriptor)
    reasons = ("qualified_mbx_source_native_abi_unavailable",
               "source_receipts_are_not_native_capabilities")
    lifetime = LifetimeEvidence(SCOPE, "unknown", "unknown", "rustc_library_only",
                                "accepted_before_close_only", "unknown", reasons)
    return JoinedEvidence(1, SCOPE, "unknown", record_descriptor["sha256"], digest,
                          None, lifetime, tuple(reports), sources)

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execution-record", type=Path, required=True)
    parser.add_argument("--expected-execution-sha256", required=True)
    parser.add_argument("--manifest", type=Path, default=Path(__file__).with_name("manifest.json"))
    parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--mbx-source-receipt-sha256")
    parser.add_argument("--compiler-source-receipt-sha256")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    def descriptor(path: Path, expected: str) -> dict[str, Any]:
        raw = regular(path, MAX_REPORT_BYTES)
        require(sha(raw) == expected, "independent artifact digest differs")
        return dict(path=str(path), size=len(raw), sha256=expected)
    sources = {name: value for name in ("mbx", "compiler")
               if (value := getattr(args, name + "_source_receipt_sha256")) is not None}
    result = join(descriptor(args.execution_record, args.expected_execution_sha256),
                  descriptor(args.manifest, args.expected_manifest_sha256), sources)
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(asdict(result), output, sort_keys=True, indent=2)
        output.write("\n")

if __name__ == "__main__":
    main()

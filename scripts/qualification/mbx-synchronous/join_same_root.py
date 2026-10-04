#!/usr/bin/env python3
"""Join fresh same-root observations through exact retained-byte origin witnesses."""
import argparse
from dataclasses import asdict, dataclass
import importlib.util
from pathlib import Path
import re
import sys
from typing import Any

SPEC = importlib.util.spec_from_file_location("same_root_base_join", Path(__file__).with_name("join.py"))
BASE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = BASE
SPEC.loader.exec_module(BASE)
MAX_RECORD_BYTES = BASE.MAX_INVENTORY_BYTES


def origin(value: dict[str, Any]) -> dict[str, Any]:
    BASE.keys(value, "path size sha256" + (" retained" if "retained" in value else ""), "origin descriptor")
    BASE.require(type(value["size"]) is int and 0 <= value["size"] <= BASE.MAX_BYTES and
                 type(value["sha256"]) is str and BASE.SHA.fullmatch(value["sha256"]), "original size/digest invalid")
    return {key: value[key] for key in ("path", "size", "sha256")}


def absolute(path: str) -> Path:
    BASE.require(type(path) is str, "original path string required")
    value = Path(path)
    BASE.require(value.is_absolute() and ".." not in value.parts and str(value) == path,
                 "absolute original path without aliases required")
    return value


class RetainedState:
    def __init__(self, value: dict[str, Any], root: Path):
        BASE.keys(value, "id original_root retained_root allocation pristine_entries pristine destroyed "
                  "directories bootstrap_inputs retained_artifacts archive_symlinks retained_ns destroyed_ns "
                  "execution_fragment lifecycle_receipt", "physical state")
        BASE.require(value["original_root"] == str(root), "state absolute root differs")
        self.value, self.root = value, root
        self.saved = absolute(value["retained_root"])
        BASE.require(self.saved == self.saved.resolve(strict=True) and self.saved.is_dir() and
                     not self.saved.is_relative_to(root) and not root.is_relative_to(self.saved),
                     "retained and active roots must be canonical and disjoint")
        records = value["retained_artifacts"]
        BASE.require(type(records) is list and len(records) <= BASE.MAX_ARTIFACTS,
                     "retained witness inventory bound/schema differs")
        self.by_origin = {}
        for item in records:
            BASE.keys(item, "path size sha256 original_device original_inode retained", "retained witness")
            origin({key: item[key] for key in ("path", "size", "sha256")})
            path = absolute(item["path"])
            BASE.require(path.is_relative_to(root) and str(path) not in self.by_origin,
                         "retained origin escapes state or repeats")
            BASE.require(all(type(item[key]) is int and item[key] >= 0
                             for key in ("original_device", "original_inode")), "original file identity invalid")
            saved = item["retained"]
            BASE.require(saved["path"] == str(self.saved / path.relative_to(root)) and
                         saved["size"] == item["size"] and saved["sha256"] == item["sha256"],
                         "retained origin bytes/path differ")
            BASE.artifact(saved)
            self.by_origin[str(path)] = item
        self.archive_links()
        wanted = {item["retained"]["path"] for item in records}
        wanted.update(str(self.saved / absolute(item["path"]).relative_to(root)) for item in value["archive_symlinks"])
        actual = {str(path) for path in self.saved.rglob("*") if path.is_symlink() or not path.is_dir()}
        BASE.require(actual == wanted, "retained physical archive coverage differs")

    def archive_links(self) -> None:
        links = self.value["archive_symlinks"]
        BASE.require(type(links) is list and len(links) <= BASE.MAX_ARTIFACTS, "archive link bound/schema differs")
        import os
        for item in links:
            BASE.keys(item, "path target", "archive symlink")
            path = absolute(item["path"])
            BASE.require(path.is_relative_to(self.root) and type(item["target"]) is str,
                         "archive link origin differs")
            saved = self.saved / path.relative_to(self.root)
            BASE.require(saved.is_symlink() and os.readlink(saved) == item["target"], "archive link differs")

    def descriptor(self, value: dict[str, Any]) -> dict[str, Any]:
        expected = origin(value)
        path = absolute(expected["path"])
        if not path.is_relative_to(self.root):
            BASE.require("retained" not in value, "nonstate artifact has retained alias")
            BASE.artifact(expected)
            return expected
        BASE.require(str(path) in self.by_origin, "original artifact lacks retained witness")
        witness = self.by_origin[str(path)]
        BASE.require(all(expected[key] == witness[key] for key in ("size", "sha256")),
                     "original descriptor differs from retained witness")
        BASE.require("retained" not in value or value["retained"] == witness["retained"],
                     "descriptor retained alias differs")
        return witness["retained"]

    def directory(self, path: str) -> Path:
        value = absolute(path)
        BASE.require(value.is_relative_to(self.root), "archived directory origin differs")
        saved = self.saved / value.relative_to(self.root)
        BASE.require(saved.is_dir() and saved == saved.resolve(strict=True), "regular archived directory required")
        return saved


def lifecycle(states: list[dict[str, Any]], root: Path, ownership: dict[str, Any]) -> None:
    BASE.keys(ownership, "path device inode nonce released_ns", "exclusive ownership")
    BASE.require(ownership["path"] == str(root.with_name(root.name + ".ownership")), "exclusive ownership path differs")
    BASE.require(all(type(ownership[key]) is int and ownership[key] >= 0 for key in ("device", "inode")) and
                 type(ownership["nonce"]) is str and re.fullmatch("[0-9a-f]{32}", ownership["nonce"]),
                 "exclusive ownership allocation invalid")
    BASE.require(type(ownership["released_ns"]) is int and ownership["released_ns"] > 0,
                 "exclusive ownership release unavailable")
    nonces, previous = set(), 0
    for index, state in enumerate(states, 1):
        BASE.require(state["id"] == index and type(state["id"]) is int and state["pristine"] is True and
                     state["pristine_entries"] == [] and state["destroyed"] is True, "fresh state lifecycle differs")
        allocation = state["allocation"]
        BASE.keys(allocation, "path device inode nonce created_ns", "state allocation")
        BASE.require(allocation["path"] == str(root) and
                     all(type(allocation[key]) is int and allocation[key] >= 0 for key in ("device", "inode")),
                     "physical root allocation differs")
        nonce = allocation["nonce"]
        BASE.require(type(nonce) is str and re.fullmatch("[0-9a-f]{32}", nonce) and nonce not in nonces,
                     "replayed state allocation nonce")
        nonces.add(nonce)
        times = (allocation["created_ns"], state["retained_ns"], state["destroyed_ns"])
        BASE.require(all(type(value) is int for value in times) and previous < times[0] <= times[1] < times[2],
                     "physical state lifecycle overlaps or lacks destruction")
        previous = times[2]
    lease = Path(ownership["path"])
    BASE.require(previous <= ownership["released_ns"] and not root.exists() and not root.is_symlink() and
                 not lease.exists() and not lease.is_symlink(),
                 "active physical state or exclusive lease remains")


def pristine_directories(state: RetainedState, run: dict[str, Any]) -> None:
    observations = state.value["directories"]
    BASE.keys(observations, "workspace cargo-home target cache home tmp rustup-home", "pristine directories")
    root = absolute(run["cwd"]).parent
    for name, item in observations.items():
        BASE.keys(item, "path device inode initial_entries", "pristine directory")
        BASE.require(item["path"] == str(root / name) and
                     all(type(item[key]) is int and item[key] >= 0 for key in ("device", "inode")),
                     "pristine directory allocation differs")
        entries = item["initial_entries"]
        BASE.require(type(entries) is list and len(entries) <= BASE.MAX_ARTIFACTS and
                     all(type(value) is str and not Path(value).is_absolute() and ".." not in Path(value).parts
                         for value in entries), "pristine entry schema differs")
        if name not in ("workspace", "cargo-home"):
            BASE.require(entries == [], "mutable state reused before import")
        elif name == "cargo-home":
            BASE.require(entries and all(value == "registry" or value.startswith("registry/") for value in entries),
                         "Cargo home includes prior mutable state")


def state_receipts(state: RetainedState, run: dict[str, Any], record: dict[str, Any]) -> None:
    value = state.value
    fragment = BASE.document(BASE.artifact(value["execution_fragment"], MAX_RECORD_BYTES))
    life = BASE.document(BASE.artifact(value["lifecycle_receipt"], MAX_RECORD_BYTES))
    BASE.require(life == {key: item for key, item in value.items() if key != "lifecycle_receipt"},
                 "retained lifecycle receipt differs")
    expected = {key: item for key, item in value.items()
                if key not in ("execution_fragment", "lifecycle_receipt", "destroyed_ns")}
    expected["destroyed"] = False
    BASE.keys(fragment, "state run tools source_receipts", "retained execution fragment")
    BASE.require(fragment == dict(state=expected, run=run, tools=record["tools"], source_receipts=record["source_receipts"]),
                 "retained execution fragment differs")


def map_command(command: dict[str, Any], state: RetainedState) -> dict[str, Any]:
    result = dict(command)
    for key in ("stdout", "stderr", "comparison_state", "bundle_inventory", "input_bundle_inventory"):
        if key in result:
            result[key] = state.descriptor(result[key])
    for key in ("report_artifacts", "admission_artifacts"):
        if key in result:
            result[key] = [state.descriptor(item) for item in result[key]]
    return result


def measured(command: dict[str, Any], run: dict[str, Any], state: RetainedState,
             record: dict[str, Any], manifest: dict[str, Any], index: int,
             sessions: set[str]) -> list[dict[str, Any]]:
    BASE.keys(command, "argv cwd environment returncode stdout stderr fixture_before fixture_after "
              "config_inputs_before config_inputs_after report_artifacts admission_artifacts", "same-root command")
    BASE.require(command["argv"] == [record["tools"]["mbx"]["path"], *manifest["commands"][index][1:]] and
                 command["cwd"] == run["cwd"] and type(command["returncode"]) is int, "same-root command route differs")
    env = command["environment"]
    BASE.keys(env, " ".join(run["environment"]) + " MBX_STATS_REPORT_DIR MBX_REPORT_CORRELATION_ID", "command environment")
    BASE.require(all(env[key] == value for key, value in run["environment"].items()), "same-root environment differs")
    correlation = record["run_attempt_id"] + ":" + str(run["id"]) + ":command-" + str(index + 1)
    BASE.require(env["MBX_REPORT_CORRELATION_ID"] == correlation, "same-root correlation differs")
    for key in ("config_inputs_before", "config_inputs_after"):
        BASE.configurations(command[key], run["cwd"], env)
    for key in ("fixture_before", "fixture_after"):
        BASE.input_snapshot(state.descriptor(command[key]), manifest, record["manifest_sha256"],
                            run["cwd"], origin(run["registry_inputs"]))
    BASE.require(all(Path(item["path"]).parent == Path(env["MBX_STATS_REPORT_DIR"])
                     for item in command["report_artifacts"]), "original report directory differs")
    mapped = map_command(command, state)
    observed = BASE.native_reports(mapped, correlation, sessions)
    for item in observed:
        item["state_id"] = run["id"]
        item["original_report_artifact"] = next(origin(value) for value in command["report_artifacts"]
                                                if value["sha256"] == item["artifact"]["sha256"])
    return observed


@dataclass(frozen=True)
class JoinedSameRoot:
    schema: int
    scope: str
    status: str
    execution_kind: str
    execution_record_sha256: str
    manifest_sha256: str
    native_authority: None
    lifetime: Any
    observed_reports: tuple[dict[str, Any], ...]
    source_receipts: dict[str, Any]
    physical_states: tuple[dict[str, Any], ...]


def reviewed_inputs(record: dict[str, Any], manifest: dict[str, Any]) -> None:
    closure = manifest["source_review"]
    BASE.require(manifest["commands"] == [["cargo", op, "--locked", "--offline", "--lib",
                 "--message-format=json-render-diagnostics"] for op in ("build", "check")] and
                 closure["fixture_build_script"] is False and closure["dependency_build_script"] is False and
                 closure["enabled_proc_macros"] == [] and closure["enabled_registry_dependencies"] == [] and
                 closure["compiled_dependency_sources"] == ["src/lib.rs", "src/u128_ext.rs"],
                 "same-root reviewed Rustc library closure differs")
    for key, expected in (("fixture_inventory_sha256", manifest["fixture"]["inventory_sha256"]),
                          ("registry_archive_sha256", manifest["registry"]["archive_sha256"]),
                          ("registry_inventory_sha256", manifest["registry"]["inventory_sha256"])):
        BASE.require(record[key] == expected, "same-root input inventory binding differs")
    BASE.keys(record["host"], "system release machine", "same-root host")
    BASE.require(all(type(value) is str and value for value in record["host"].values()), "same-root host identity required")
    BASE.require(type(record["run_attempt_id"]) is str and record["run_attempt_id"], "same-root attempt identity required")


def join(record_descriptor: dict[str, Any], manifest_descriptor: dict[str, Any],
         expected_sources: dict[str, str] | None = None) -> JoinedSameRoot:
    record = BASE.document(BASE.artifact(record_descriptor, MAX_RECORD_BYTES))
    manifest = BASE.document(BASE.artifact(manifest_descriptor, BASE.MAX_REPORT_BYTES))
    fields = "schema scope execution_kind run_attempt_id status runs native_authority native_abi hosted_t01_t03 " + \
        "host limitations manifest_sha256 source_receipts fixture_inventory_sha256 registry_archive_sha256 " + \
        "registry_inventory_sha256 toolchain_inventory tools registry_seed_inventory tool_observations artifacts " + \
        "same_root_states active_root exclusive_ownership"
    BASE.keys(record, fields, "same-root execution")
    BASE.require(record["schema"] == 1 and type(record["schema"]) is int and record["scope"] == manifest["scope"] == BASE.SCOPE
                 and record["execution_kind"] == "local_same_root" and record["status"] == "observed", "same-root schema/status differs")
    BASE.require(record["native_authority"] is None and record["native_abi"] is None and record["hosted_t01_t03"] is None,
                 "same-root parsed native authority flag")
    BASE.require(record["manifest_sha256"] == manifest_descriptor["sha256"], "same-root reviewed manifest differs")
    reviewed_inputs(record, manifest)
    root = absolute(record["active_root"])
    BASE.require(root == root.resolve(strict=False), "same-root canonical active path required")
    states, runs = record["same_root_states"], record["runs"]
    BASE.require(type(states) is list and type(runs) is list and len(states) == len(runs) == 3, "three physical states required")
    lifecycle(states, root, record["exclusive_ownership"])
    BASE.keys(record["tools"], "mbx cargo rustc", "same-root tools")
    for value in record["tools"].values():
        BASE.artifact(value)
    BASE.inventory(record["toolchain_inventory"], Path(record["tools"]["rustc"]["path"]).parent.parent)
    sources = BASE.retained_sources(record["source_receipts"], expected_sources or {})
    reports, sessions = [], set()
    same_cwd = str(root / "run-1/workspace")
    for state_value, run in zip(states, runs, strict=True):
        state = RetainedState(state_value, root)
        BASE.keys(run, "id cwd environment config_inputs registry_archive registry_source registry_inputs commands transport", "same-root run")
        BASE.require(run["id"] == state_value["id"] and run["cwd"] == same_cwd, "same absolute command root differs")
        state_receipts(state, run, record)
        pristine_directories(state, run)
        BASE.input_snapshot(state.descriptor(state.value["bootstrap_inputs"]), manifest, record["manifest_sha256"],
                            run["cwd"], origin(run["registry_inputs"]))
        BASE.controlled_environment(run["environment"], record["tools"])
        BASE.configurations(run["config_inputs"], run["cwd"], run["environment"])
        BASE.inventory_bytes(manifest["fixture"]["files"], state.directory(run["cwd"]),
                             fixture_lock_alias=True)
        BASE.inventory(state.descriptor(run["registry_inputs"]), state.directory(str(Path(run["cwd"]).parent / "cargo-home/registry")))
        BASE.require(run["registry_archive"] in state.by_origin, "archived registry archive witness missing")
        archive = state.by_origin[run["registry_archive"]]["retained"]
        BASE.require(archive["sha256"] == manifest["registry"]["archive_sha256"], "archived registry origin differs")
        registry_files = sorted(manifest["registry"]["files"] + [manifest["registry"]["local_extraction_marker"]], key=lambda item: item["path"])
        BASE.inventory_bytes(registry_files, state.directory(run["registry_source"]))
        mapped_transport = [map_command(value, state) for value in run["transport"]]
        BASE.auxiliary(mapped_transport, run, record, manifest)
        BASE.require(type(run["commands"]) is list and len(run["commands"]) == 2, "same-root declared commands unavailable")
        for index, command in enumerate(run["commands"]):
            reports.extend(measured(command, run, state, record, manifest, index, sessions))
        if run["id"] == 1:
            BASE.auxiliary([map_command(value, state) for value in record["tool_observations"]], run, record, manifest, True)
            BASE.require(origin(record["registry_seed_inventory"]) == origin(run["registry_inputs"]), "registry seed witness differs")
    for value in record["artifacts"]:
        BASE.artifact(value)
    lifetime = BASE.LifetimeEvidence(BASE.SCOPE, "unknown", "unknown", "rustc_library_only",
                                    "accepted_before_close_only", "unknown",
                                    ("qualified_mbx_source_native_abi_unavailable", "physical_state_observations_are_not_native_capabilities"))
    return JoinedSameRoot(1, BASE.SCOPE, "unknown", "local_same_root", record_descriptor["sha256"],
                          record["manifest_sha256"], None, lifetime, tuple(reports), sources, tuple(states))


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
        data = BASE.regular(path, MAX_RECORD_BYTES)
        BASE.require(BASE.sha(data) == expected, "same-root independent artifact digest differs")
        return dict(path=str(path), size=len(data), sha256=expected)
    sources = {name: value for name in ("mbx", "compiler")
               if (value := getattr(args, name + "_source_receipt_sha256")) is not None}
    result = join(descriptor(args.execution_record, args.expected_execution_sha256),
                  descriptor(args.manifest, args.expected_manifest_sha256), sources)
    with args.output.open("x", encoding="utf-8") as destination:
        import json
        json.dump(asdict(result), destination, sort_keys=True, indent=2)
        destination.write("\n")


if __name__ == "__main__":
    main()

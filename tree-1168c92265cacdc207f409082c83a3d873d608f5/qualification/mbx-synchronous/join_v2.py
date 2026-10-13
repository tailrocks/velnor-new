#!/usr/bin/env python3
"""Join original local evidence, then validate only the native public transport contract."""
import argparse
from dataclasses import asdict
import importlib.util
import json
from pathlib import Path
import sys
from typing import Any

ROOT = Path(__file__).resolve().parent


def module(name: str, filename: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, ROOT / filename)
    value = importlib.util.module_from_spec(spec)
    sys.modules[name] = value
    spec.loader.exec_module(value)
    return value


RELOCATED = module("v2_relocated_join", "join.py")
SAME = module("v2_same_root_join", "join_same_root.py")
PROTOCOL = module("v2_cache_transport", "cache_transport_v2.py")


def transport_stdout(record: dict[str, Any], index: int, command: dict[str, Any],
                     layout: str) -> dict[str, Any]:
    value = command["stdout"]
    if layout == "relocated":
        return value
    expected = SAME.origin(value)
    records = record["same_root_states"][index]["retained_artifacts"]
    matches = [item for item in records if item["path"] == expected["path"]]
    RELOCATED.require(len(matches) == 1, "V2 original transport stdout witness unavailable")
    witness = matches[0]
    RELOCATED.require(all(witness[key] == expected[key] for key in ("size", "sha256")) and
                      value.get("retained", witness["retained"]) == witness["retained"],
                      "V2 transport retained origin differs")
    return witness["retained"]


def public_transport(record: dict[str, Any], layout: str) -> list[dict[str, Any]]:
    runs = record["runs"]
    RELOCATED.require(record["status"] == "observed" and type(runs) is list and len(runs) == 3,
                      "V2 requires three completed local observations")
    observations = []
    for index, run in enumerate(runs):
        RELOCATED.require(type(run["commands"]) is list and len(run["commands"]) == 2,
                          "V2 measured fixture command closure differs")
        commands = run["transport"]
        operations = [command["argv"][2] for command in commands]
        RELOCATED.require(operations == ["comparison-state" if index == 0 else "import", "export"],
                          "V2 local transport sequence differs")
        for command, operation in zip(commands, operations, strict=True):
            RELOCATED.require(type(command["returncode"]) is int and command["returncode"] == 0,
                              "V2 local transport command failed")
            descriptor = transport_stdout(record, index, command, layout)
            raw = RELOCATED.artifact(descriptor, PROTOCOL.MAX_BYTES)
            value = PROTOCOL.validate(operation, raw)
            if operation == "comparison-state":
                RELOCATED.require(value["empty"] is True, "V2 cold comparison baseline differs")
            if operation == "import":
                RELOCATED.require(value["comparison_state_recorded"] is True, "V2 import baseline unavailable")
            if operation == "export":
                RELOCATED.require(value["budget_refused"] is False, "V2 successful observation refused export budget")
                RELOCATED.require(value == command["export_observed"], "V2 native export bytes differ from routing observation")
            observations.append(dict(run_id=run["id"], operation=operation, argv=command["argv"],
                                     cwd=command["cwd"], original_stdout=SAME.origin(command["stdout"]),
                                     retained_stdout=descriptor, native_report=value))
    return observations


def join(envelope_descriptor: dict[str, Any], manifest_descriptor: dict[str, Any],
         expected_execution_sha256: str, expected_validator_sha256: str,
         expected_sources: dict[str, str] | None = None) -> dict[str, Any]:
    envelope = RELOCATED.document(RELOCATED.artifact(envelope_descriptor, SAME.MAX_RECORD_BYTES))
    RELOCATED.keys(envelope, "schema scope layout transport_api validator driver execution_record", "V2 execution envelope")
    RELOCATED.require(type(envelope["schema"]) is int and envelope["schema"] == 2 and
                      envelope["scope"] == RELOCATED.SCOPE and envelope["layout"] in ("relocated", "same-root"),
                      "unsupported V2 execution envelope")
    RELOCATED.keys(envelope["transport_api"], "export import comparison", "V2 transport API")
    expected_api = {"export": 2, "import": 1, "comparison": 1}
    RELOCATED.require(all(type(value) is int and value == expected_api[name]
                          for name, value in envelope["transport_api"].items()), "V2 transport API differs")
    validator = envelope["validator"]
    RELOCATED.require(validator["path"] == str(ROOT / "cache_transport_v2.py") and
                      validator["sha256"] == expected_validator_sha256, "V2 independent validator identity differs")
    RELOCATED.artifact(validator)
    layout = envelope["layout"]
    driver = envelope["driver"]
    filename = "run.py" if layout == "relocated" else "run_same_root.py"
    RELOCATED.require(driver["path"] == str(ROOT / filename), "V2 execution driver differs")
    RELOCATED.artifact(driver)
    execution = envelope["execution_record"]
    RELOCATED.require(execution["sha256"] == expected_execution_sha256, "V2 independent execution identity differs")
    record = RELOCATED.document(RELOCATED.artifact(execution, SAME.MAX_RECORD_BYTES))
    owner = RELOCATED if layout == "relocated" else SAME
    result = owner.join(execution, manifest_descriptor, expected_sources)
    observed = public_transport(record, layout)
    RELOCATED.require(result.native_authority is None and result.status == "unknown", "V2 cannot mint native authority")
    RELOCATED.artifact(validator)
    return dict(schema=2, scope=RELOCATED.SCOPE, status="unknown", native_authority=None,
                layout=layout, envelope_sha256=envelope_descriptor["sha256"],
                validator=validator, driver=driver, transport_api=expected_api,
                observed_transport=observed, original_join=asdict(result))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--envelope", type=Path, required=True)
    parser.add_argument("--expected-envelope-sha256", required=True)
    parser.add_argument("--expected-execution-sha256", required=True)
    parser.add_argument("--expected-validator-sha256", required=True)
    parser.add_argument("--manifest", type=Path, default=ROOT / "manifest.json")
    parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--mbx-source-receipt-sha256")
    parser.add_argument("--compiler-source-receipt-sha256")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    def descriptor(path: Path, expected: str) -> dict[str, Any]:
        raw = RELOCATED.regular(path, SAME.MAX_RECORD_BYTES)
        RELOCATED.require(RELOCATED.sha(raw) == expected, "V2 independent artifact digest differs")
        return dict(path=str(path), size=len(raw), sha256=expected)
    sources = {name: value for name in ("mbx", "compiler")
               if (value := getattr(args, name + "_source_receipt_sha256")) is not None}
    result = join(descriptor(args.envelope, args.expected_envelope_sha256),
                  descriptor(args.manifest, args.expected_manifest_sha256), args.expected_execution_sha256,
                  args.expected_validator_sha256, sources)
    with args.output.open("x", encoding="utf-8") as output:
        json.dump(result, output, sort_keys=True, indent=2)
        output.write("\n")


if __name__ == "__main__":
    main()

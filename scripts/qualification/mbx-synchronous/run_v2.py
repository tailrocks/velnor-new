#!/usr/bin/env python3
"""Run bounded local three-state drivers with strict native public transport V2."""

import argparse
from contextlib import contextmanager
import importlib.util
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def metadata(arguments):
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--layout", choices=("relocated", "same-root"), required=True)
    parser.add_argument("--expected-validator-sha256", required=True)
    options, remainder = parser.parse_known_args(arguments)
    output_parser = argparse.ArgumentParser(add_help=False)
    output_parser.add_argument("--output", type=Path, required=True)
    output, _ = output_parser.parse_known_args(remainder)
    return options, remainder, output.output


def owned_transport(argv):
    values = [str(value) for value in argv]
    if len(values) >= 3 and values[1] == "cache":
        operation = values[2]
        if operation in ("export", "import", "comparison-state"):
            return operation
    return None


@contextmanager
def strict_execution(base, validator):
    original = base.execute

    def execute(argv, cwd, environment, logs, name):
        record = original(argv, cwd, environment, logs, name)
        operation = owned_transport(argv)
        if operation is not None:
            try:
                if record["returncode"] == 0:
                    with Path(record["stdout"]["path"]).open("rb") as source:
                        raw = source.read(validator.MAX_BYTES + 1)
                    validator.validate(operation, raw)
                else:
                    raise ValueError("native transport exited unsuccessfully")
            except (ValueError, OSError, KeyError) as error:
                # The driver retains the original logs; this records failed route identity.
                base.write_json(logs / (name + ".transport-validation.json"),
                                dict(operation=operation, command=record, error=str(error),
                                     native_authority=None))
                raise ValueError("strict V2 " + operation + " validation failed: " + str(error)) from error
        return record

    base.execute = execute
    try:
        yield
    finally:
        base.execute = original


def bind_envelope(base, output, layout, validator_path, driver_path):
    execution = base.artifact(output / "execution.json")
    record = json.loads(Path(execution["path"]).read_bytes())
    envelope = dict(schema=2, scope="exact_synchronous_fixture_v1", layout=layout,
                    transport_api={"export": 2, "import": 1, "comparison": 1},
                    validator=base.artifact(validator_path), driver=base.artifact(driver_path),
                    execution_record=execution)
    base.write_json(output / "execution-v2.json", envelope)
    return record


def main():
    options, remainder, output = metadata(sys.argv[1:])
    driver_path = ROOT / ("run_same_root.py" if options.layout == "same-root" else "run.py")
    driver = load("v2_execution_driver", driver_path)
    base = driver.BASE if options.layout == "same-root" else driver
    validator_path = ROOT / "cache_transport_v2.py"
    base.require(base.artifact(validator_path)["sha256"] == options.expected_validator_sha256,
                 "reviewed public transport validator digest differs")
    validator = load("v2_public_transport", validator_path)
    initial_driver = base.artifact(driver_path)
    original_argv = sys.argv
    try:
        sys.argv = [original_argv[0], *remainder]
        with strict_execution(base, validator):
            code = driver.main()
    finally:
        sys.argv = original_argv
    base.require(base.artifact(validator_path)["sha256"] == options.expected_validator_sha256,
                 "public transport validator changed during execution")
    base.require(base.artifact(driver_path) == initial_driver, "execution driver changed during execution")
    record = bind_envelope(base, output, options.layout, validator_path, driver_path)
    print(json.dumps(dict(status=record["status"], envelope=str(output / "execution-v2.json"),
                          native_authority=None)))
    return code


if __name__ == "__main__":
    sys.exit(main())

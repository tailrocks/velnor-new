"""Select a binary from the compiler-artifact records emitted by Cargo."""

import argparse
import json
import os
from pathlib import Path


def select_executable(messages, workspace_root, manifest_path, target_name):
    """Return the executable Cargo built for the selected workspace target."""
    root = Path(workspace_root).resolve(strict=True)
    manifest = Path(manifest_path)
    if not manifest.is_absolute():
        manifest = root / manifest
    manifest = manifest.resolve(strict=True)
    matches = []

    for line_number, line in enumerate(messages, start=1):
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid Cargo JSON at line {line_number}") from error
        if not isinstance(record, dict) or record.get("reason") != "compiler-artifact":
            continue
        target = record.get("target")
        if not isinstance(target, dict) or target.get("name") != target_name:
            continue
        kind = target.get("kind")
        if not isinstance(kind, list) or "bin" not in kind:
            continue
        emitted_manifest = record.get("manifest_path")
        if not isinstance(emitted_manifest, str):
            continue
        try:
            emitted_manifest = Path(emitted_manifest).resolve(strict=True)
        except OSError:
            continue
        if emitted_manifest != manifest:
            continue

        executable = record.get("executable")
        if not isinstance(executable, str) or not executable:
            raise ValueError(f"Cargo reported no executable for {target_name}")
        path = Path(executable)
        if not path.is_absolute():
            path = root / path
        try:
            path = path.resolve(strict=True)
        except OSError as error:
            raise ValueError(f"Cargo executable does not exist: {path}") from error
        if not path.is_file() or not os.access(path, os.X_OK):
            raise ValueError(f"Cargo executable is not executable: {path}")
        matches.append(path)

    if len(matches) != 1:
        raise ValueError(
            f"expected one Cargo executable for {target_name}, found {len(matches)}"
        )
    return matches[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("messages", type=Path)
    parser.add_argument("--workspace-root", required=True)
    parser.add_argument("--manifest-path", required=True)
    parser.add_argument("--target", required=True)
    arguments = parser.parse_args()
    try:
        with arguments.messages.open(encoding="utf-8") as messages:
            print(
                select_executable(
                    messages,
                    arguments.workspace_root,
                    arguments.manifest_path,
                    arguments.target,
                )
            )
    except (OSError, ValueError) as error:
        parser.exit(1, f"{parser.prog}: {error}\n")


if __name__ == "__main__":
    main()

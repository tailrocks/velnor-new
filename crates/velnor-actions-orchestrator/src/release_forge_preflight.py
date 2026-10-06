"""Verify the anonymous package artifact against forge and registry state."""

import os
import re
import subprocess
import zipfile


def create_forge_preflight():
    """Consume one exact package artifact and emit publication preflight evidence."""
    approved = policy()
    candidate, _archives, _artifact, _blob = load_package_input(approved)
    packages = candidate["packages"]
    for name, version in approved["packages"].items():
        forge_package(approved, name, False)
        existing = fetch(f"https://crates.io/api/v1/crates/{name}/{version}", 2 * 1024 * 1024)
        if existing is not None:
            registry_package(approved, name, version, packages[name])
    evidence = {
        "schema": 1,
        "policy": approved,
        "packages": packages,
        "publication_order": candidate["publication_order"],
        "workflow_sha": os.environ["GITHUB_SHA"],
        "run_id": os.environ["GITHUB_RUN_ID"],
        "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
        "status": "publication-incomplete",
        "operations": {
            name: {"version": version, "status": "pending"}
            for name, version in approved["packages"].items()
        },
    }
    save_receipt(evidence, "release-preflight/evidence.json")


def forge_preflight_main():
    try:
        create_forge_preflight()
    except (
        ReconcileError,
        OSError,
        ValueError,
        KeyError,
        TypeError,
        zipfile.BadZipFile,
        subprocess.SubprocessError,
    ) as error:
        raise SystemExit(
            f"release_forge_preflight:{type(error).__name__}:{str(error)[:160]}"
        ) from error


if __name__ == "__main__":
    forge_preflight_main()

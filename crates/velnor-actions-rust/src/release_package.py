"""Create anonymous Cargo package evidence from the approved source checkout."""

import os
from pathlib import Path
import re
import subprocess


PACKAGE_EVIDENCE_STATUS = "package-verified"


def _run_identity(approved):
    source_sha = os.environ.get("GITHUB_SHA", "")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "")
    require(re.fullmatch(r"[0-9a-f]{40}", source_sha) and
            re.fullmatch(r"[1-9][0-9]*", run_id) and
            re.fullmatch(r"[1-9][0-9]*", attempt), "package_run_identity")
    return source_sha, run_id, attempt


def verify_source_checkout(source, expected_sha):
    """Bind Cargo's package source to one clean exact Git checkout."""
    environment = _clean_environment()
    argv = ["git", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=" + os.devnull]
    result = subprocess.run(
        [*argv, "rev-parse", "HEAD"],
        cwd=source,
        env=environment,
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    require(result.stdout.strip() == expected_sha, "source_checkout_sha")
    result = subprocess.run(
        [*argv, "status", "--porcelain"],
        cwd=source,
        env=environment,
        capture_output=True,
        text=True,
        check=True,
        timeout=30,
    )
    require(not result.stdout, "dirty_source")


def create_package_evidence():
    """Verify Cargo packages anonymously before writing candidate evidence."""
    approved = policy()
    _run_identity(approved)
    validate_source()
    source = Path("release-source").resolve(strict=True)
    verify_source_checkout(source, approved["source_sha"])
    manifest = _safe_relative_manifest(os.environ["RELEASE_MANIFEST"])
    packages = approved_package_inventory(approved, source, manifest)
    evidence = {
        "schema": 1,
        "policy": approved,
        "packages": packages,
        "publication_order": selected_publication_order(packages),
        "workflow_sha": os.environ["GITHUB_SHA"],
        "run_id": os.environ["GITHUB_RUN_ID"],
        "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
        "status": PACKAGE_EVIDENCE_STATUS,
    }
    save_receipt(evidence, "release-package/evidence.json")


def package_main():
    try:
        create_package_evidence()
    except (ReconcileError, ValidationError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit(f"release_package:{type(error).__name__}:{str(error)[:160]}") from error


if __name__ == "__main__":
    package_main()

"""Fixed local protocol fixtures; no network or package execution."""
import io
import json
import os
from pathlib import Path
import tarfile
import zipfile


class ReconcileError(ValueError):
    pass


def require(condition, reason):
    if not condition:
        raise ReconcileError(reason)


ROOT = Path(__file__).resolve().parents[1] / "src"
NS = {"__name__": "publish_test", "require": require,
      "ReconcileError": ReconcileError, "decode_json": json.loads}
COMMON = ROOT.parents[1] / "velnor-actions-orchestrator/src/release_reconcile_common.py"
exec(compile(COMMON.read_text(), str(COMMON), "exec"), NS)
ReconcileError, require = NS["ReconcileError"], NS["require"]
for filename in ("release_reconcile_cargo.py", "release_package_contract.py", "release_reconcile_registry.py",
                 "release_publish_metadata.py", "release_publish_manifest.py", "release_publish_transport.py",
                 "release_publish_auth.py", "release_publish_verify.py", "release_publish_artifact.py",
                 "release_publish_registry.py"):
    exec(compile((ROOT / filename).read_text(), filename, "exec"), NS)
SHA = "a" * 40
ENVIRONMENT = {"GITHUB_SHA": SHA, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2"}
POLICY = {"source_sha": SHA, "packages": {"demo": "1.0.0"},
          "owners": {"demo": ["user:1"]}, "authentication": "bootstrap-token"}


def package(name="demo", version="1.0.0"):
    files = {"Cargo.toml": f'[package]\nname="{name}"\nversion="{version}"\n'.encode(),
             ".cargo_vcs_info.json": json.dumps({"git": {"sha1": SHA}, "path_in_vcs": ""}).encode(),
             "src/lib.rs": b"pub fn fixture() {}\n"}
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as archive:
        for path, data in files.items():
            member = tarfile.TarInfo(f"{name}-{version}/{path}")
            member.size = len(data)
            archive.addfile(member, io.BytesIO(data))
    data = output.getvalue()
    metadata = {key: None for key in ("description", "documentation", "homepage", "readme",
                "readme_file", "license", "license_file", "repository", "links", "rust_version")}
    metadata.update(name=name, vers=version, deps=[], features={}, authors=[], keywords=[],
                    categories=[], badges={})
    proof = NS["inventory"](data, name, version, SHA)
    proof.update(archive_sha256=NS["hashlib"].sha256(data).hexdigest(),
                 publish_metadata=metadata, dependencies=[], cargo_dependency_proofs=[],
                 forge_release={"tag_name": f"{name}-v{version}", "name": f"{name} {version}",
                                "body": "Fixture changelog", "draft": False, "prerelease": False})
    return proof, data


def registry_proof(package, checksum=None):
    checksum = checksum or package["archive_sha256"]
    return {"status": "verified", "registry_checksum": checksum, "archive_checksum": checksum,
            "owners": ["user:1"], "files": package["files"], "features": package["features"]}


def artifact(proofs, archives, approved=POLICY, overrides=None, extra=None):
    evidence = {"schema": 1, "policy": approved, "packages": proofs,
                "workflow_sha": SHA, "run_id": "123", "run_attempt": "2",
                "status": "package-verified", "publication_order": sorted(proofs)}
    evidence.update(overrides or {})
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("evidence.json", json.dumps(evidence))
        for name, data in archives.items():
            archive.writestr(f"crates/{name}-{approved['packages'][name]}.crate", data)
        if extra:
            archive.writestr(*extra)
    return stream.getvalue()

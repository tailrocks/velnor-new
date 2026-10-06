"""Pure immutable package artifact validation for credential-bearing consumers."""
import hashlib
import io
import os
from pathlib import PurePosixPath
import re
import stat
import tarfile
import zipfile


def _archive_contents(archive, name, version):
    contents = {}
    prefix = f"{name}-{version}/"
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as package:
        for member in package:
            stream = package.extractfile(member)
            require(stream is not None, "publish_manifest_stream")
            content = stream.read(member.size + 1)
            require(len(content) == member.size, "publish_manifest_truncated")
            contents[member.name[len(prefix):]] = content
    return contents


def validate_registry_inputs(approved, packages, archives):
    require(isinstance(packages, dict) and isinstance(archives, dict) and
            set(packages) == set(archives) == set(approved["packages"]), "publish_package_scope")
    for name, version in approved["packages"].items():
        expected, archive = packages[name], archives[name]
        validate_package_shape(expected)
        require(type(archive) is bytes and len(archive) <= 64 * 1024 * 1024, "publish_archive_type")
        require(expected["archive_sha256"] == hashlib.sha256(archive).hexdigest(),
                "publish_archive_checksum")
        actual = inventory(archive, name, version, approved["source_sha"])
        require(actual == {key: expected[key] for key in ("files", "features")},
                "publish_archive_source")
        validate_publish_metadata(expected["publish_metadata"], name, version)
        validate_archive_publish_metadata(_archive_contents(archive, name, version),
                expected["publish_metadata"], name, version, expected["cargo_dependency_proofs"])
        require(expected["publish_metadata"]["features"] == actual["features"],
                "publish_archive_features")
        dependencies = sorted({item["name"] for item in expected["publish_metadata"]["deps"]
                               if item["kind"] != "dev" and item["name"] in packages})
        require(expected["dependencies"] == dependencies, "publish_dependency_order_proof")
    return selected_publication_order(packages)


def validate_package_artifact(blob, approved):
    require(type(blob) is bytes and len(blob) <= 256 * 1024 * 1024,
            "package_artifact_size")
    names = approved["packages"]
    require(all(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", name) and
                re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?",
                             version) for name, version in names.items()), "package_artifact_identity")
    expected = {"evidence.json", *(f"crates/{name}-{version}.crate"
                                  for name, version in names.items())}
    files = {}
    total = 0
    with zipfile.ZipFile(io.BytesIO(blob)) as archive:
        for member in archive.infolist():
            path = PurePosixPath(member.filename)
            require(member.filename in expected and member.filename not in files and
                    str(path) == member.filename and not path.is_absolute() and
                    ".." not in path.parts and "\\" not in member.filename and
                    not member.is_dir() and not member.flag_bits & 1,
                    "package_artifact_members")
            mode = member.external_attr >> 16
            require(stat.S_IFMT(mode) in (0, stat.S_IFREG), "package_artifact_nonregular")
            bound = 16 * 1024 * 1024 if member.filename == "evidence.json" else 64 * 1024 * 1024
            require(member.file_size <= bound, "package_artifact_member_size")
            total += member.file_size
            require(total <= 256 * 1024 * 1024, "package_artifact_expansion")
            with archive.open(member) as stream:
                content = stream.read(bound + 1)
            require(len(content) == member.file_size, "package_artifact_truncated")
            files[member.filename] = content
    require(set(files) == expected, "package_artifact_member_scope")
    candidate = decode_json(files["evidence.json"])
    fields = {"schema", "policy", "packages", "workflow_sha", "run_id", "run_attempt",
              "status", "publication_order"}
    require(isinstance(candidate, dict) and set(candidate) == fields and
            type(candidate["schema"]) is int and candidate["schema"] == 1 and
            same_json(candidate["policy"], approved) and
            candidate["status"] == "package-verified", "package_artifact_fields")
    require(candidate["workflow_sha"] == os.environ["GITHUB_SHA"] and
            candidate["run_id"] == os.environ["GITHUB_RUN_ID"] and
            candidate["run_attempt"] == os.environ["GITHUB_RUN_ATTEMPT"],
            "package_artifact_run")
    archives = {name: files[f"crates/{name}-{version}.crate"] for name, version in names.items()}
    order = validate_registry_inputs(approved, candidate["packages"], archives)
    require(candidate["publication_order"] == order, "package_artifact_publication_order")
    return candidate, archives

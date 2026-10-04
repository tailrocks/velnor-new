#!/usr/bin/env python3
"""Verify exact fixture/registry bytes; bind opaque execution evidence, never authority."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import tarfile

ROOT = Path(__file__).resolve().parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def regular(path):
    require(path.is_file() and not path.is_symlink(), "regular file required: " + str(path))
    return path.read_bytes()


def inventory(directory):
    require(directory.is_dir() and not directory.is_symlink(), "regular source directory required")
    result = []
    for path in sorted(directory.rglob("*")):
        require(not path.is_symlink(), "source symlink forbidden")
        if path.is_dir():
            continue
        data = regular(path)
        result.append(dict(path=path.relative_to(directory).as_posix(),
                           size=len(data), sha256=sha(data)))
    return result


def reject_symlink_components(path):
    require(path.is_absolute(), "fixture root must be absolute")
    current = Path(path.anchor)
    for component in path.parts[1:]:
        if component == "..":
            current = current.parent
            continue
        if component in ("", "."):
            continue
        current /= component
        require(not current.is_symlink(), "fixture root path contains symlink component")


def canonical_fixture_root(path, repository=None, normalize=False):
    require(path.is_absolute(), "fixture root must be an absolute canonical path")
    reject_symlink_components(path)
    lexical = Path(os.path.normpath(str(path)))
    reject_symlink_components(lexical)
    if not normalize:
        require(path == lexical, "fixture root must be an absolute canonical path")
    if repository is not None:
        require(lexical.is_relative_to(repository), "fixture root escapes the repository")
    canonical = lexical.resolve(strict=True)
    require(lexical == canonical,
            "fixture root must be an absolute canonical path without symlink ancestors")
    if repository is not None:
        require(canonical.is_relative_to(repository.resolve(strict=True)),
                "fixture root escapes the repository")
    return canonical


def fixture_source(manifest):
    root = manifest["fixture"]["root"]
    require(type(root) is str and root, "manifest fixture root must be a relative path")
    relative = Path(root)
    require(not relative.is_absolute(), "manifest fixture root must be relative")
    repository = ROOT.parents[2]
    return canonical_fixture_root(ROOT / relative, repository=repository, normalize=True)


def copy_fixture(source, destination):
    """Copy the reviewed fixture and materialize its locked Cargo workspace."""
    source_records = inventory(source)
    shutil.copytree(source, destination)
    paths = [destination, *sorted(destination.rglob("*"))]
    for path in sorted(paths, key=lambda item: (len(item.parts), item.as_posix())):
        require(not path.is_symlink(), "copied fixture symlink forbidden")
        require(path.is_dir() or path.is_file(), "copied fixture special file forbidden")
        path.chmod(0o700 if path.is_dir() else 0o600)
    require(inventory(destination) == source_records, "copied fixture bytes differ")
    fixture_lock = destination / "Cargo.lock.fixture"
    cargo_lock = destination / "Cargo.lock"
    require(fixture_lock.is_file() and not fixture_lock.is_symlink(),
            "fixture Cargo.lock.fixture required")
    require(not cargo_lock.exists() and not cargo_lock.is_symlink(),
            "fixture Cargo.lock destination already exists")
    fixture_lock.rename(cargo_lock)
    return destination


def inventory_sha(records):
    return sha(json.dumps(records, sort_keys=True, separators=(",", ":")).encode())


def fixture_root(args, manifest):
    path = getattr(args, "fixture_root", None)
    if path is None:
        return fixture_source(manifest)
    return canonical_fixture_root(path)


def fixture_inventory(directory, expected):
    records = inventory(directory)
    expected_paths = {item["path"] for item in expected}
    if "Cargo.lock.fixture" in expected_paths:
        lock_paths = {item["path"] for item in records
                      if item["path"] in ("Cargo.lock", "Cargo.lock.fixture")}
        require(len(lock_paths) == 1, "fixture must contain one pinned Cargo lock")
        if "Cargo.lock" in lock_paths:
            records = [dict(item, path="Cargo.lock.fixture") if item["path"] == "Cargo.lock"
                       else item for item in records]
    return records


def verify_registry(spec, archive, source):
    require(sha(regular(archive)) == spec["archive_sha256"], "registry archive checksum differs")
    records = []
    prefix = spec["name"] + "-" + spec["version"] + "/"
    with tarfile.open(archive, "r:gz") as stream:
        for member in sorted(stream.getmembers(), key=lambda item: item.name):
            require(member.isfile() and member.name.startswith(prefix), "unsupported archive member")
            relative = member.name[len(prefix):]
            require(relative and ".." not in Path(relative).parts, "invalid archive path")
            data = stream.extractfile(member).read()
            require(regular(source / relative) == data, "registry extraction differs: " + relative)
            records.append(dict(path=relative, size=len(data), sha256=sha(data)))
    require(records == spec["files"], "registry archive inventory differs")
    require(inventory_sha(records) == spec["inventory_sha256"], "registry inventory digest differs")
    expected = sorted(records + [spec["local_extraction_marker"]], key=lambda item: item["path"])
    require(inventory(source) == expected, "unexpected registry source input")


def verify(args):
    raw = regular(ROOT / "manifest.json")
    require(sha(raw) == args.expected_manifest_sha256, "reviewed manifest digest differs")
    manifest = json.loads(raw)
    require(manifest["scope"] == "exact_synchronous_fixture_v1", "unexpected scope")
    fixture = manifest["fixture"]
    records = fixture_inventory(fixture_root(args, manifest), fixture["files"])
    require(records == fixture["files"], "unexpected or changed fixture input")
    require(inventory_sha(records) == fixture["inventory_sha256"], "fixture inventory digest differs")
    verify_registry(manifest["registry"], args.registry_archive, args.registry_source)
    return manifest, sha(raw)


def bind(args, manifest, manifest_sha):
    require(args.execution_record is not None and args.artifact,
            "binding requires owner execution record and artifacts")
    record = regular(args.execution_record)
    # Deliberately opaque: only the owning native session can evaluate its proof.
    artifacts = []
    for path in args.artifact:
        data = regular(path)
        artifacts.append(dict(path=str(path.resolve()), size=len(data), sha256=sha(data)))
    require(len({item["path"] for item in artifacts}) == len(artifacts), "duplicate artifact")
    result = dict(schema=1, scope=manifest["scope"], status="fixture-input-binding-only",
                  manifest_sha256=manifest_sha,
                  binder_sha256=sha(regular(Path(__file__))),
                  fixture_root=str(fixture_root(args, manifest)),
                  fixture_inventory_sha256=manifest["fixture"]["inventory_sha256"],
                  registry_archive_sha256=manifest["registry"]["archive_sha256"],
                  registry_inventory_sha256=manifest["registry"]["inventory_sha256"],
                  execution_record=dict(path=str(args.execution_record.resolve()),
                                        size=len(record), sha256=sha(record)),
                  execution_artifacts=artifacts, native_authority=None,
                  limitations=manifest["limitations"])
    with args.output.open("x", encoding="utf-8") as destination:
        json.dump(result, destination, indent=2, sort_keys=True)
        destination.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected-manifest-sha256", required=True)
    parser.add_argument("--registry-archive", type=Path, required=True)
    parser.add_argument("--registry-source", type=Path, required=True)
    parser.add_argument("--fixture-root", type=Path,
                        help="absolute canonical executed copy; default: committed fixture")
    parser.add_argument("--execution-record", type=Path)
    parser.add_argument("--artifact", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    manifest, manifest_sha = verify(args)
    if args.output is not None:
        bind(args, manifest, manifest_sha)
    print(json.dumps(dict(status="fixture-inputs-verified", manifest_sha256=manifest_sha,
                          fixture_root=str(fixture_root(args, manifest)),
                          native_authority=None)))


if __name__ == "__main__":
    main()

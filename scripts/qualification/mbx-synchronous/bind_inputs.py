#!/usr/bin/env python3
"""Verify exact fixture/registry bytes; bind opaque execution evidence, never authority."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
from pathlib import PurePosixPath
import tarfile

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[2]


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


def relative_path(value, label):
    require(type(value) is str and value, label + " path must be a string")
    path = PurePosixPath(value)
    require(path.parts and "\\" not in value and not path.is_absolute() and
            path.as_posix() == value and
            all(part not in ("", ".", "..") for part in path.parts),
            label + " path must be canonical and relative")
    return path


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key: " + key)
        result[key] = value
    return result


def fixture_material(manifest):
    spec = manifest["fixture"]["material"]
    relative = relative_path(spec["path"], "fixture material")
    require(relative.as_posix().startswith(
        "crates/mbx-synchronous-registry-fixture/tests/fixtures/"),
        "fixture material must be crate test data")
    path = REPO.joinpath(*relative.parts)
    require(path.resolve(strict=True) == path.absolute(), "fixture material symlink forbidden")
    raw = regular(path)
    require(len(raw) == spec["size"] and sha(raw) == spec["sha256"],
            "fixture material digest differs")
    material = json.loads(raw, object_pairs_hook=unique_object)
    require(material == dict(schema=1, scope="standalone_workspace_fixture_v1",
                             materialization="private_tmpdir", files=material["files"]),
            "unexpected fixture material schema")
    files, records = {}, []
    for entry in material["files"]:
        relative_file = relative_path(entry["path"], "fixture file")
        name = relative_file.as_posix()
        require(name not in files, "duplicate fixture file: " + name)
        if entry["kind"] == "base64":
            require(set(entry) == {"path", "kind", "contents", "size", "sha256"},
                    "invalid embedded fixture fields")
            data = base64.b64decode(entry["contents"], validate=True)
        elif entry["kind"] == "repository_file":
            require(set(entry) == {"path", "kind", "source", "size", "sha256"},
                    "invalid repository fixture fields")
            source = relative_path(entry["source"], "fixture source")
            require(source.as_posix().startswith("crates/mbx-synchronous-registry-fixture/"),
                    "fixture Rust source must live in its registered crate")
            source_path = REPO.joinpath(*source.parts)
            require(source_path.resolve(strict=True) == source_path.absolute(),
                    "fixture source symlink forbidden")
            data = regular(source_path)
        else:
            raise ValueError("unknown fixture material encoding")
        record = dict(path=name, size=len(data), sha256=sha(data))
        require(record["size"] == entry["size"] and record["sha256"] == entry["sha256"],
                "fixture material file digest differs: " + name)
        files[name] = data
        records.append(record)
    records.sort(key=lambda item: item["path"])
    require(records == manifest["fixture"]["files"], "fixture material inventory differs")
    require(inventory_sha(records) == manifest["fixture"]["inventory_sha256"],
            "fixture material inventory digest differs")
    return files, records


def materialize_fixture(destination, manifest, private_root=None):
    files, expected = fixture_material(manifest)
    require(destination.is_absolute() and not destination.exists() and not destination.is_symlink(),
            "new absolute fixture directory required")
    require(destination.parent.resolve(strict=True) == destination.parent,
            "fixture parent must be canonical")
    if private_root is not None:
        require(private_root.is_absolute() and private_root.resolve(strict=True) == private_root,
                "private TMPDIR must be absolute and canonical")
        info = private_root.stat()
        require(info.st_uid == os.getuid() and info.st_mode & 0o077 == 0 and
                private_root in destination.parents,
                "fixture must be inside an owned private TMPDIR")
    destination.mkdir(mode=0o700)
    info = destination.stat()
    require(info.st_uid == os.getuid() and info.st_mode & 0o077 == 0,
            "fixture directory must be owned and private")
    for name, data in sorted(files.items()):
        target = destination.joinpath(*PurePosixPath(name).parts)
        target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        require(target.parent.resolve(strict=True) == target.parent,
                "materialized fixture directory symlink forbidden")
        with target.open("xb") as output:
            output.write(data)
        target.chmod(0o600)
    require(inventory(destination) == expected, "materialized fixture differs")
    return destination


def fixture_root(args, manifest):
    path = getattr(args, "fixture_root", None)
    require(path is not None, "executed materialized fixture root is required")
    require(path.is_absolute() and path == path.resolve(strict=True),
            "fixture root must be an absolute canonical path without symlink ancestors")
    return path


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
    _, expected = fixture_material(manifest)
    root = getattr(args, "fixture_root", None)
    if root is not None:
        records = inventory(fixture_root(args, manifest))
        require(records == expected, "unexpected or changed fixture input")
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
                        help="absolute canonical materialized execution copy")
    parser.add_argument("--execution-record", type=Path)
    parser.add_argument("--artifact", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    manifest, manifest_sha = verify(args)
    if args.output is not None:
        bind(args, manifest, manifest_sha)
    print(json.dumps(dict(status="fixture-inputs-verified", manifest_sha256=manifest_sha,
                          fixture_root=(str(args.fixture_root) if args.fixture_root is not None else None),
                          native_authority=None)))


if __name__ == "__main__":
    main()

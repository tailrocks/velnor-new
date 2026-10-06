"""Trusted pre-execution Cargo preparation; deliberately unregistered until qualified."""
import hashlib
from collections.abc import Mapping
import io
import json
import os
from pathlib import Path
import selectors
import subprocess
import tarfile
import tempfile
import time
from types import MappingProxyType


_INTENT_HOME_SEAL = object()


class _IntentCargoHome:
    __slots__ = ("path", "identity", "_seal")

    def __init__(self, seal, path):
        _intent_require(seal is _INTENT_HOME_SEAL and not any(path.iterdir()),
                        "source_intent_cargo_home_fresh")
        observed = path.stat()
        object.__setattr__(self, "path", path)
        object.__setattr__(self, "identity", (observed.st_dev, observed.st_ino))
        object.__setattr__(self, "_seal", seal)

    def __setattr__(self, name, value):
        raise AttributeError("immutable Cargo home")


class _IntentEnvironment(Mapping):
    __slots__ = ("_variables", "cargo_home")

    def __init__(self, variables, cargo_home):
        object.__setattr__(self, "_variables", MappingProxyType(dict(variables)))
        object.__setattr__(self, "cargo_home", cargo_home)

    def __setattr__(self, name, value):
        raise AttributeError("immutable Cargo environment")

    def __getitem__(self, key):
        return self._variables[key]

    def __iter__(self):
        return iter(self._variables)

    def __len__(self):
        return len(self._variables)


def _intent_dependency(name):
    dependency = globals().get(name)
    _intent_require(callable(dependency), "source_intent_dependency_unavailable:" + name)
    return dependency


def _intent_sdk(sdk):
    sdk_type = _intent_dependency("ColdSourceIntentSdk")
    _intent_require(type(sdk) is sdk_type, "source_intent_sdk_capability")
    tool_type = _intent_dependency("ColdSourceIntentInstalledTool")
    paths = []
    for tool in ("cargo", "rustc"):
        proof = sdk.installed_tool(tool)
        _intent_require(type(proof) is tool_type, "source_intent_sdk_tool_capability")
        proof.require_current()
        path, digest = Path(proof.path), proof.sha256
        _intent_require(path.is_absolute() and not path.is_symlink() and path.is_file() and
                        path.resolve(strict=True) == path, "source_intent_sdk_path")
        _intent_require(isinstance(digest, str) and len(digest) == 64 and
                        hashlib.sha256(path.read_bytes()).hexdigest() == digest,
                        "source_intent_sdk_digest")
        proof.require_current()
        paths.append(path)
    return paths


def _intent_cargo_home(cargo_home):
    _intent_require(type(cargo_home) is _IntentCargoHome and
                    cargo_home._seal is _INTENT_HOME_SEAL,
                    "source_intent_cargo_home_capability")
    path = cargo_home.path
    _intent_require(path.is_absolute() and not path.is_symlink() and path.is_dir() and
                    path.resolve(strict=True) == path, "source_intent_cargo_home")
    observed = path.stat()
    _intent_require((observed.st_dev, observed.st_ino) == cargo_home.identity,
                    "source_intent_cargo_home_identity")
    forbidden = {"config", "config.toml", "credentials", "credentials.toml"}
    _intent_require(not any(item.name.casefold() in forbidden for item in path.iterdir()),
                    "source_intent_cargo_home_configuration")
    return path


def _intent_environment(directory, cargo_home=None):
    locations = {}
    for name in ("home", "cargo-home", "rustup-home", "empty-path", "cwd"):
        if name == "cargo-home" and cargo_home is not None:
            locations[name] = _intent_cargo_home(cargo_home)
            continue
        location = directory / name
        location.mkdir()
        locations[name] = location
        if name == "cargo-home":
            cargo_home = _IntentCargoHome(_INTENT_HOME_SEAL, location)
    _intent_ancestors(locations["cwd"])
    return _IntentEnvironment({"HOME": str(locations["home"]),
            "CARGO_HOME": str(locations["cargo-home"]),
            "RUSTUP_HOME": str(locations["rustup-home"]),
            "PATH": str(locations["empty-path"]), "LC_ALL": "C", "TZ": "UTC"}, cargo_home)


def _intent_current_environment(environment):
    _intent_require(type(environment) is _IntentEnvironment,
                    "source_intent_environment_capability")
    path = _intent_cargo_home(environment.cargo_home)
    _intent_require(environment["CARGO_HOME"] == str(path), "source_intent_cargo_home_binding")


def _intent_capture(argv, directory, environment, limit=8 * 1024 * 1024, timeout=600):
    _intent_current_environment(environment)
    selector = selectors.DefaultSelector()
    try:
        process = subprocess.Popen(argv, cwd=directory / "cwd", env=environment, shell=False,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE)
    except BaseException:
        selector.close()
        raise
    output, sizes = bytearray(), {"stdout": 0, "stderr": 0}
    deadline = time.monotonic() + timeout
    try:
        selector.register(process.stdout, selectors.EVENT_READ, "stdout")
        selector.register(process.stderr, selectors.EVENT_READ, "stderr")
        while selector.get_map():
            _intent_require(time.monotonic() < deadline, "source_intent_cargo_timeout")
            for key, _event in selector.select(min(0.1, max(0, deadline - time.monotonic()))):
                data = os.read(key.fileobj.fileno(), min(65536, limit + 1 - sizes[key.data]))
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                sizes[key.data] += len(data)
                _intent_require(sizes[key.data] <= limit, "source_intent_cargo_output")
                if key.data == "stdout":
                    output.extend(data)
        status = process.wait(timeout=max(0.01, deadline - time.monotonic()))
        _intent_require(status == 0, "source_intent_cargo_failed")
        return bytes(output)
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()
        selector.close()
        process.stdout.close()
        process.stderr.close()


def _intent_command(sdk, arguments, directory, environment):
    cargo, rustc = _intent_sdk(sdk)
    argv = [str(cargo), *arguments, "--config", "build.rustc=" + json.dumps(str(rustc)),
            "--config", 'build.rustc-wrapper=""',
            "--config", 'build.rustc-workspace-wrapper=""']
    return _intent_capture(argv, directory, environment)


def _intent_materialize(source, approved, destination):
    _intent_dependency("authenticated_source_snapshot")(source, 16 * 1024 * 1024)
    _intent_require(source.repository == approved["repository"] and
                    source.source_sha == approved["source_sha"], "source_intent_identity")
    # Owner checks every case/Unicode/component collision before the first write.
    _intent_dependency("materialize_authenticated_source_snapshot")(source, destination)
    return guard_source_intent(destination)


def _intent_archive_contents(data, name, version, destination):
    # The independent pure inventory must run before extraction.
    item = _intent_dependency("source_intent_content_inventory")(data, name, version)
    contents = {}
    prefix = f"{name}-{version}/"
    destination.mkdir(parents=True)
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for member in archive:
            relative = member.name[len(prefix):]
            stream = archive.extractfile(member)
            _intent_require(stream is not None, "source_intent_archive_stream")
            content = stream.read()
            contents[relative] = content
            path = destination / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("xb") as output:
                output.write(content)
    guard_source_intent(destination)
    return item, contents


def _intent_canonical_package(data, name, version, sdk, cargo_home):
    with tempfile.TemporaryDirectory(prefix="velnor-intent-metadata-",
                                     dir=Path("/tmp").resolve(strict=True)) as temporary:
        directory = Path(temporary)
        environment = _intent_environment(directory, cargo_home)
        root = directory / "source"
        item, contents = _intent_archive_contents(data, name, version, root)
        raw = _intent_command(sdk, ["metadata", "--locked", "--offline", "--no-deps",
                              "--format-version", "1", "--manifest-path",
                              str(root / "Cargo.toml")], directory, environment)
        metadata = _intent_dependency("approved_publish_metadata")(
            contents, json.loads(raw), name, version)
        proofs = _intent_dependency("archive_dependency_proofs")(contents, metadata)
        _intent_dependency("validate_archive_publish_metadata")(
            contents, metadata, name, version, proofs)
        item.update(archive_sha256=hashlib.sha256(data).hexdigest(),
                    publish_metadata=metadata, cargo_dependency_proofs=proofs)
        return item


def source_intent_inventory(approved, manifest, sdk, actual_host, destination, release_config,
                            source_descriptor):
    """Prepare original bytes and official metadata before any repository execution.

    Authenticated source and private RootLinux SDK are mandatory compiled dependencies.
    Caller must run this stage in its own fresh zero-token job.
    """
    _intent_require(type(sdk) is _intent_dependency("ColdSourceIntentSdk"),
                    "source_intent_sdk_capability")
    sdk.require_policy(approved["tools"]["rust"], actual_host)
    _intent_sdk(sdk)
    source = _intent_dependency("load_authenticated_source_snapshot")()
    observed_source = _intent_dependency("authenticated_source_descriptor")(source)
    _intent_require(_intent_dependency("same_json")(observed_source, source_descriptor),
                    "source_intent_source_artifact_changed")
    inventory_api = _intent_dependency("source_intent_content_inventory")
    _intent_require(callable(inventory_api), "source_intent_content_inventory_unavailable")
    manifest = _intent_dependency("_safe_relative_manifest")(str(manifest))
    with tempfile.TemporaryDirectory(prefix="velnor-intent-package-",
                                     dir=Path("/tmp").resolve(strict=True)) as temporary:
        directory = Path(temporary)
        environment = _intent_environment(directory)
        root = _intent_materialize(source, approved, directory / "source")
        arguments = ["package", "--locked", "--no-verify", "--allow-dirty",
                     "--manifest-path", str(root / manifest),
                     "--target-dir", str(directory / "target")]
        for name in sorted(approved["packages"]):
            arguments.extend(["--package", name])
        _intent_command(sdk, arguments, directory, environment)
        full_metadata = json.loads(_intent_command(sdk,
            ["metadata", "--locked", "--offline", "--no-deps", "--format-version", "1",
             "--manifest-path", str(root / manifest)], directory, environment))
        descriptors = _intent_dependency("forge_release_intent")(
            full_metadata, source, {"approved": approved, "release_config": release_config}, str(root))
        _intent_require(type(descriptors) is dict and
                        set(descriptors) == set(approved["packages"]),
                        "source_intent_forge_descriptor_packages")
        packages, originals = {}, {}
        for name, version in approved["packages"].items():
            archive = directory / "target/package" / f"{name}-{version}.crate"
            _intent_require(archive.is_file() and not archive.is_symlink(),
                            "source_intent_archive_missing")
            data = archive.read_bytes()
            packages[name] = _intent_canonical_package(
                data, name, version, sdk, environment.cargo_home)
            packages[name]["dependencies"] = sorted({item["name"]
                for item in packages[name]["publish_metadata"]["deps"]
                if item["kind"] != "dev" and item["name"] in approved["packages"]})
            packages[name]["forge_release"] = descriptors[name]
            originals[name] = data
        _intent_dependency("selected_publication_order")(packages)
        output = Path(destination) / "crates"
        output.mkdir(parents=True, exist_ok=False)
        for name, data in originals.items():
            path = output / f"{name}-{approved['packages'][name]}.crate"
            with path.open("xb") as stream:
                stream.write(data)
            path.chmod(0o444)
    return packages

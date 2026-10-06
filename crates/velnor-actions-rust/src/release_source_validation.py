"""Validate the selected package set in the approved exact-source checkout.

This helper is emitted as a generator-owned support file and runs with a
pinned Python/Rust tool pair. It invokes Cargo through an argv list only; the
metadata probe never evaluates repository code or shell text.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from typing import Any
from urllib.parse import unquote, urlsplit


class ValidationError(ValueError):
    """A source package or publication-policy invariant failed."""


class _ObjectPairs(list[tuple[Any, Any]]):
    """Marker preserving that the expected JSON value was an object."""


PACKAGE_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$")
REGISTRY_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$")
VERSION = re.compile(
    r"^[0-9]+\.[0-9]+\.[0-9]+"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)
REPOSITORY = re.compile(r"^[A-Za-z0-9._-]{1,100}/[A-Za-z0-9._-]{1,100}$")

SOURCE_DIR = "release-source"
METADATA_ARGS = (
    "cargo",
    "metadata",
    "--locked",
    "--no-deps",
    "--format-version",
    "1",
    "--config",
    'build.rustc="rustc"',
    "--config",
    'build.rustc-wrapper=""',
    "--config",
    'build.rustc-workspace-wrapper=""',
)

EXACT_CREDENTIAL_KEYS = frozenset(
    {
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_ID_TOKEN_REQUEST_URL",
        "ACTIONS_RUNTIME_TOKEN",
        "CARGO_REGISTRY_GLOBAL_CREDENTIAL_PROVIDERS",
        "CARGO_REGISTRY_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "MISE_GITHUB_TOKEN",
        "NODE_AUTH_TOKEN",
        "NPM_TOKEN",
    }
)
EXECUTION_OVERRIDE_KEYS = frozenset(
    {
        "RUSTC",
        "RUSTDOC",
        "RUSTUP_TOOLCHAIN",
        "CARGO_BUILD_RUSTC",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTDOCFLAGS",
        "RUSTFLAGS",
    }
)
COMMAND_FILE_KEYS = frozenset(
    {
        "GITHUB_ENV",
        "GITHUB_PATH",
        "GITHUB_OUTPUT",
        "GITHUB_STATE",
        "GITHUB_STEP_SUMMARY",
    }
)


def _fail(reason: str) -> None:
    raise ValidationError(reason)


def _text(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value:
        _fail(f"invalid_{field}")
    return value


def _safe_relative_manifest(value: str) -> Path:
    path = Path(value)
    if (
        not value
        or path.is_absolute()
        or "\\" in value
        or "//" in value
        or value.startswith("./")
        or "/./" in value
        or value.startswith("../")
        or "/../" in value
        or path.name != "Cargo.toml"
        or any(part in ("", ".", "..") for part in path.parts)
    ):
        _fail("invalid_manifest")
    return path


def _reject_symlink_components(root: Path, relative: Path, field: str) -> None:
    current = root
    for part in relative.parts:
        current /= part
        if current.is_symlink():
            _fail(f"symlink_{field}")


def _canonical_path(value: Any, field: str) -> Path:
    raw = _text(value, field)
    path = Path(raw)
    if not path.is_absolute():
        _fail(f"noncanonical_{field}")
    try:
        resolved = path.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        _fail(f"unresolvable_{field}:{error}")
    if str(resolved) != raw:
        _fail(f"noncanonical_{field}")
    return resolved


def _path_from_package_id(value: Any) -> tuple[Path, str]:
    package_id = _text(value, "package_id")
    source, separator, fragment = package_id.partition("#")
    if not separator or not source.startswith("path+"):
        _fail("non_path_package_id")
    parsed = urlsplit(source[5:])
    if (
        parsed.scheme != "file"
        or parsed.netloc not in ("", "localhost")
        or parsed.query
        or parsed.fragment
    ):
        _fail("noncanonical_package_id")
    path = _canonical_path(unquote(parsed.path), "package_id_path")
    return path, unquote(fragment)


def _clean_environment() -> dict[str, str]:
    environment = dict(os.environ)
    for key in list(environment):
        if (
            key in EXACT_CREDENTIAL_KEYS
            or key in COMMAND_FILE_KEYS
            or key.startswith("ACTIONS_")
            or key.startswith("ACTIONS_ID_TOKEN_REQUEST_")
            or key.startswith("CARGO_REGISTRIES_")
            or key.startswith("CARGO_REGISTRY_")
            or key.startswith("GIT_")
            or key.endswith("_TOKEN")
            or key in EXECUTION_OVERRIDE_KEYS
        ):
            environment.pop(key, None)
    rust_toolchain = environment.get("RELEASE_RUST_TOOLCHAIN")
    if not isinstance(rust_toolchain, str) or not VERSION.fullmatch(rust_toolchain):
        _fail("invalid_rust_toolchain")
    environment["RUSTUP_TOOLCHAIN"] = rust_toolchain
    environment["GIT_CONFIG_NOSYSTEM"] = "1"
    environment["GIT_CONFIG_GLOBAL"] = os.devnull
    return environment


def _run_metadata(manifest: Path) -> dict[str, Any]:
    argv = [*METADATA_ARGS, "--manifest-path", str(manifest)]
    with tempfile.TemporaryDirectory(prefix="velnor-cargo-metadata-") as directory:
        cargo_home = Path(directory) / "cargo-home"
        cargo_home.mkdir()
        environment = _clean_environment()
        environment["CARGO_HOME"] = str(cargo_home)
        result = subprocess.run(
            argv,
            cwd=directory,
            env=environment,
            capture_output=True,
            check=False,
            shell=False,
            text=True,
            timeout=120,
        )
    if result.returncode != 0:
        detail = (result.stderr or "").splitlines()
        suffix = detail[0][:160] if detail else "cargo_failed"
        _fail(f"metadata_failed:{suffix}")
    try:
        document = json.loads(result.stdout)
    except (TypeError, json.JSONDecodeError) as error:
        _fail(f"metadata_invalid_json:{error}")
    if not isinstance(document, dict) or document.get("version") != 1:
        _fail("metadata_version")
    return document


def _workspace_packages(document: dict[str, Any], source_root: Path) -> dict[str, dict[str, Any]]:
    workspace_root = _canonical_path(document.get("workspace_root"), "workspace_root")
    if not workspace_root.is_relative_to(source_root):
        _fail("workspace_root_outside_source")
    if not workspace_root.is_dir():
        _fail("workspace_root_not_directory")
    raw_packages = document.get("packages")
    members = document.get("workspace_members")
    if (
        not isinstance(raw_packages, list)
        or not isinstance(members, list)
        or any(not isinstance(member, str) or not member for member in members)
    ):
        _fail("metadata_workspace_shape")
    if not members or len(set(members)) != len(members):
        _fail("metadata_workspace_members")
    by_id: dict[str, dict[str, Any]] = {}
    for raw in raw_packages:
        if not isinstance(raw, dict):
            _fail("metadata_package_shape")
        package_id = _text(raw.get("id"), "package_id")
        if package_id in by_id:
            _fail("duplicate_package_id")
        manifest = _canonical_path(raw.get("manifest_path"), "manifest_path")
        if (
            not manifest.is_relative_to(source_root)
            or manifest.name != "Cargo.toml"
            or not manifest.is_file()
        ):
            _fail("manifest_outside_source")
        id_path, fragment = _path_from_package_id(package_id)
        if id_path != manifest.parent:
            _fail("package_id_path_mismatch")
        name = _text(raw.get("name"), "package_name")
        version = _text(raw.get("version"), "package_version")
        if not PACKAGE_NAME.fullmatch(name) or not VERSION.fullmatch(version):
            _fail("invalid_package_identity")
        compact_id = fragment == version and id_path.name == name
        qualified_id = fragment == f"{name}@{version}"
        if not compact_id and not qualified_id:
            _fail("package_id_identity_mismatch")
        by_id[package_id] = raw
    member_set = set(members)
    if member_set != set(by_id):
        _fail("workspace_package_set_mismatch")
    by_name: dict[str, dict[str, Any]] = {}
    for package_id in members:
        package = by_id.get(package_id)
        if package is None:
            _fail("workspace_member_missing")
        name = _text(package.get("name"), "package_name")
        if name in by_name:
            _fail("duplicate_package_name")
        by_name[name] = package
    return by_name


def _publishable(package: dict[str, Any], registry: str) -> bool:
    policy = package.get("publish")
    if policy is None:
        return registry == "crates-io"
    if not isinstance(policy, list):
        _fail("invalid_publish_policy")
    if not policy:
        return False
    if any(not isinstance(item, str) or not REGISTRY_NAME.fullmatch(item) for item in policy):
        _fail("invalid_publish_policy")
    if len(set(policy)) != len(policy):
        _fail("duplicate_publish_registry")
    return registry in policy


def _expected_map() -> dict[str, str]:
    raw = os.environ.get("RELEASE_EXPECTED_PACKAGES")
    if raw is None:
        _fail("missing_expected_packages")
    try:
        value = json.loads(
            raw,
            object_pairs_hook=_ObjectPairs,
            parse_constant=lambda constant: _fail(
                f"nonfinite_expected_packages:{constant}"
            ),
        )
    except json.JSONDecodeError as error:
        _fail(f"invalid_expected_packages:{error}")
    if not isinstance(value, _ObjectPairs) or not value:
        _fail("invalid_expected_packages")
    expected: dict[str, str] = {}
    for pair in value:
        if not isinstance(pair, (list, tuple)) or len(pair) != 2:
            _fail("invalid_expected_packages")
        name, version = pair
        if (
            not isinstance(name, str)
            or not PACKAGE_NAME.fullmatch(name)
            or not isinstance(version, str)
            or not VERSION.fullmatch(version)
        ):
            _fail("invalid_expected_package_identity")
        if name in expected:
            _fail("duplicate_expected_package")
        expected[name] = version
    return expected


def validate_source() -> None:
    """Validate exact-source package identities and publication scope."""
    registry = os.environ.get("RELEASE_REGISTRY", "")
    if not REGISTRY_NAME.fullmatch(registry):
        _fail("invalid_registry")
    repository = os.environ.get("RELEASE_REPOSITORY")
    if repository is not None and not REPOSITORY.fullmatch(repository):
        _fail("invalid_repository")
    source_root_input = Path(SOURCE_DIR)
    if source_root_input.is_symlink():
        _fail("symlink_source_root")
    source_root = source_root_input
    try:
        source_root = source_root.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        _fail(f"missing_source_root:{error}")
    if not source_root.is_dir():
        _fail("source_root_not_directory")
    manifest_relative = _safe_relative_manifest(os.environ.get("RELEASE_MANIFEST", ""))
    _reject_symlink_components(source_root, manifest_relative, "manifest")
    try:
        manifest = (source_root / manifest_relative).resolve(strict=True)
    except (OSError, RuntimeError) as error:
        _fail(f"missing_manifest:{error}")
    if (
        not manifest.is_relative_to(source_root)
        or manifest.name != "Cargo.toml"
        or not manifest.is_file()
    ):
        _fail("manifest_outside_source")
    document = _run_metadata(manifest)
    actual = _workspace_packages(document, source_root)
    expected = _expected_map()
    missing = sorted(set(expected) - set(actual))
    if missing:
        _fail("missing_selected:" + ",".join(missing))
    for name, version in expected.items():
        actual_version = actual[name].get("version")
        if actual_version != version:
            _fail(f"version_mismatch:{name}")
        if not _publishable(actual[name], registry):
            _fail(f"publish_policy:{name}")
    scope = os.environ.get("RELEASE_PUBLISHABLE_WORKSPACE")
    if scope not in ("0", "1"):
        _fail("invalid_publishable_workspace")
    if scope == "1":
        publishable = {
            name: package["version"]
            for name, package in actual.items()
            if _publishable(package, registry)
        }
        if publishable != expected:
            missing_scope = sorted(set(publishable) - set(expected))
            missing_scope.extend(sorted(set(expected) - set(publishable)))
            _fail("publishable_scope_mismatch:" + ",".join(missing_scope))


def main() -> None:
    """Run the fixed validation and emit one bounded diagnostic on failure."""
    try:
        validate_source()
    except (ValidationError, OSError, subprocess.SubprocessError) as error:
        detail = str(error)[:160]
        raise SystemExit(
            f"release_source_validation:{type(error).__name__}:{detail}"
        ) from error


if __name__ == "__main__":
    main()

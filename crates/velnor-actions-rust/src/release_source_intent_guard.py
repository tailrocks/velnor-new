"""Closed filesystem/source policy before trusted Cargo reads repository data."""
import os
from pathlib import Path
import stat
import tomllib


class SourceIntentError(ValueError):
    """Trusted source preparation cannot satisfy its closed policy."""


def _intent_require(condition, reason):
    if not condition:
        raise SourceIntentError(reason)


def _intent_ancestors(path):
    for ancestor in (path, *path.parents):
        for relative in (".git", ".cargo/config", ".cargo/config.toml"):
            _intent_require(not (ancestor / relative).exists() and
                            not (ancestor / relative).is_symlink(),
                            "source_intent_ambient_configuration")
        manifest = ancestor / "Cargo.toml"
        _intent_require(ancestor == path or
                        (not manifest.exists() and not manifest.is_symlink()),
                        "source_intent_ambient_workspace")


def _intent_path(root, parent, value):
    _intent_require(isinstance(value, str) and value and "\\" not in value and
                    not Path(value).is_absolute(), "source_intent_path")
    _intent_require(not (".." in Path(value).parts and
                         any(character in value for character in "*?[")),
                    "source_intent_parent_glob_unsupported")
    # Resolve containment only; glob matching and package membership belong to Cargo.
    candidate = (parent / value).resolve(strict=False)
    _intent_require(candidate.is_relative_to(root), "source_intent_path_escape")


def _intent_paths(root, parent, document):
    package = document.get("package", {})
    workspace = document.get("workspace", {})
    _intent_require("default_members" not in workspace,
                    "source_intent_legacy_workspace_path")
    for table in (package, workspace.get("package", {})):
        _intent_require("license_file" not in table, "source_intent_legacy_package_path")
        for field in ("workspace", "build", "readme", "license-file"):
            value = table.get(field)
            if value is not None and type(value) is not bool and not isinstance(value, dict):
                _intent_path(root, parent, value)
    for field in ("members", "exclude", "default-members"):
        for value in workspace.get(field, []):
            _intent_path(root, parent, value)
    targets = [document.get("lib", {})]
    for field in ("bin", "example", "test", "bench"):
        targets.extend(document.get(field, []))
    for target in targets:
        if "path" in target:
            _intent_path(root, parent, target["path"])


def _intent_dependencies(root, parent, document):
    _intent_require(not document.get("cargo-features") and
                    "patch" not in document and "replace" not in document,
                    "source_intent_unsupported_resolution")
    tables = [document, document.get("workspace", {}),
              *document.get("target", {}).values()]
    fields = {"version", "package", "features", "optional", "default-features",
              "default_features", "workspace", "path", "registry", "registry-index"}
    for table in tables:
        _intent_require(not {"build_dependencies", "dev_dependencies"}.intersection(table),
                        "source_intent_legacy_dependency_heading")
        for heading in ("dependencies", "build-dependencies", "dev-dependencies"):
            for dependency in table.get(heading, {}).values():
                if isinstance(dependency, str):
                    continue
                _intent_require(isinstance(dependency, dict) and
                                set(dependency) <= fields,
                                "source_intent_dependency_source")
                _intent_require(dependency.get("registry") in (None, "crates-io") and
                                dependency.get("registry-index") in
                                (None, "https://github.com/rust-lang/crates.io-index"),
                                "source_intent_dependency_registry")
                if "path" in dependency:
                    _intent_path(root, parent, dependency["path"])


def _intent_walk_error(error):
    raise SourceIntentError("source_intent_incomplete_walk") from error


def _guard_source_intent(root):
    """Reject resolution/execution escapes without interpreting Cargo semantics."""
    root = Path(root)
    _intent_require(root.is_absolute() and root.resolve(strict=True) == root and
                    root.is_dir() and not root.is_symlink(), "source_intent_root")
    _intent_ancestors(root)
    manifests = []
    for directory, directories, files in os.walk(root, followlinks=False,
                                                onerror=_intent_walk_error):
        for name in [*directories, *files]:
            path = Path(directory) / name
            mode = path.lstat().st_mode
            _intent_require(not path.is_symlink() and
                            (stat.S_ISDIR(mode) or stat.S_ISREG(mode)),
                            "source_intent_nonregular")
            _intent_require(name.lower() not in (".git", ".cargo_vcs_info.json"),
                            "source_intent_git_metadata")
            _intent_require(not (name.lower() in ("config", "config.toml") and
                                path.parent.name.lower() == ".cargo"),
                            "source_intent_cargo_configuration")
            if name.lower() == "cargo.toml" and stat.S_ISREG(mode):
                manifests.append(path)
            if name.lower() == "cargo.lock" and stat.S_ISREG(mode):
                lock = tomllib.loads(path.read_text(encoding="utf-8"))
                for package in lock.get("package", []):
                    _intent_require(package.get("source") in
                                    (None, "registry+https://github.com/rust-lang/crates.io-index"),
                                    "source_intent_lock_source")
    _intent_require(bool(manifests), "source_intent_manifest_missing")
    for manifest in manifests:
        document = tomllib.loads(manifest.read_text(encoding="utf-8"))
        _intent_dependencies(root, manifest.parent, document)
        _intent_paths(root, manifest.parent, document)
    return root


def guard_source_intent(root):
    """Fail closed on unsupported or malformed raw filesystem/TOML inputs."""
    try:
        return _guard_source_intent(root)
    except SourceIntentError:
        raise
    except (OSError, RuntimeError, ValueError, TypeError, AttributeError) as error:
        raise SourceIntentError("source_intent_malformed_source") from error

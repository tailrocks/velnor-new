"""Closed version-only manifest edits, shared by proposal and forge proofs."""
import copy
import posixpath
import re
import tomllib

_NUMBER = r"(?:0|[1-9][0-9]*)"
_PRERELEASE = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
VERSION = re.compile(
    rf"{_NUMBER}\.{_NUMBER}\.{_NUMBER}"
    rf"(?:-{_PRERELEASE}(?:\.{_PRERELEASE})*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?")
_COMPARATOR = re.compile(
    rf"(>=|<=|=|>|<|\^|~)? *({_NUMBER})"
    rf"(?:\.({_NUMBER}|[xX*]))?(?:\.({_NUMBER}|[xX*]))?"
    rf"(?:-({_PRERELEASE}(?:\.{_PRERELEASE})*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))? *")
_U64_MAX = 18446744073709551615


def _require_u64(value):
    require(len(value) <= 20 and int(value) <= _U64_MAX, "proposal_version_overflow")


def _parse_requirement(value):
    """Mirror semver 1.0.28 VersionReq parsing into Comparator fields."""
    require(isinstance(value, str) and value, "proposal_version_type")
    if value.strip(" ") in ("*", "x", "X"):
        return []
    parts = value.split(",")
    require(len(parts) <= 32, "proposal_requirement_comparators")
    result = []
    for part in parts:
        parsed = _COMPARATOR.fullmatch(part.lstrip(" "))
        require(parsed is not None, "proposal_version_syntax")
        operator, major, minor, patch, prerelease, build = parsed.groups()
        wild_minor, wild_patch = minor in ("*", "x", "X"), patch in ("*", "x", "X")
        require(not wild_minor or patch is None or wild_patch, "proposal_version_syntax")
        require(not (prerelease or build) or patch is not None and not wild_patch,
                "proposal_version_syntax")
        for number in (major, minor, patch):
            if number is not None and number not in ("*", "x", "X"):
                _require_u64(number)
        result.append((operator if operator is not None else
                       ("" if wild_minor or wild_patch else "^"),
                       major, None if wild_minor else minor,
                       None if wild_patch else patch, prerelease or ""))
    return result


def _require_requirement(value):
    _parse_requirement(value)


def _require_version(value, requirement=False):
    require(isinstance(value, str) and value, "proposal_version_type")
    if requirement:
        _require_requirement(value)
    else:
        require(VERSION.fullmatch(value), "proposal_version_syntax")
        for number in value.split("+", 1)[0].split("-", 1)[0].split("."):
            _require_u64(number)


def _dependency_versions(table, selected):
    if not isinstance(table, dict):
        return
    for name, dependency in table.items():
        if not isinstance(dependency, dict) or "path" not in dependency:
            continue
        package = dependency.get("package", name)
        if selected is not None and package not in selected:
            continue
        if "version" in dependency:
            version = dependency["version"]
            _require_version(version, True)
            dependency["version"] = "<version>"


def _manifest_shape(document, selected=None):
    value = copy.deepcopy(document)
    package = value.get("package", {})
    if isinstance(package.get("version"), str) and (selected is None or package.get("name") in selected):
        _require_version(package["version"])
        package["version"] = "<version>"
    workspace = value.get("workspace", {})
    workspace_package = workspace.get("package", {})
    if isinstance(workspace_package.get("version"), str):
        _require_version(workspace_package["version"])
        workspace_package["version"] = "<version>"
    for table in [value, workspace, *value.get("target", {}).values()]:
        for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
            _dependency_versions(table.get(kind, {}), selected)
    return value


def preparation_lock_versions(raw, packages_after):
    """Bind workspace lock records to independently resolved manifest versions."""
    document = tomllib.loads(raw.decode("utf-8"))
    records = document.get("package", [])
    require(isinstance(records, list), "proposal_lock_packages")
    seen = set()
    for package in records:
        require(isinstance(package, dict), "proposal_lock_package")
        name = package.get("name")
        if "source" in package:
            continue
        for dependency in package.get("dependencies", []):
            require(isinstance(dependency, str), "proposal_lock_dependency")
            parts = dependency.split(" ")
            if len(parts) == 2 and parts[0] in packages_after:
                require(parts[1] == packages_after[parts[0]], "proposal_lock_dependency_version")
        if name not in packages_after:
            continue
        require(name not in seen, "proposal_lock_duplicate_workspace_package")
        seen.add(name)
        version = package.get("version")
        _require_version(version)
        _require_version(packages_after[name])
        require(version == packages_after[name], "proposal_lock_package_version")


def _upgrade_requirement(value, version):
    """Mirror pinned cargo_utils upgrade and semver 1.0.28 Comparator Display."""
    comparators = _parse_requirement(value)
    _require_version(version)
    if not comparators:
        return value
    core = version.split("+", 1)[0]
    numbers, separator, prerelease = core.partition("-")
    next_parts = numbers.split(".")
    upgraded = []
    for operator, _, minor, patch, _ in comparators:
        require(operator in ("", "=", "~", "^"), "proposal_requirement_unsupported")
        rendered = operator + next_parts[0]
        if minor is not None:
            rendered += "." + next_parts[1]
            if patch is not None:
                rendered += "." + next_parts[2]
                if separator:
                    rendered += "-" + prerelease
            elif operator == "":
                rendered += ".*"
        elif operator == "":
            rendered += ".*"
        upgraded.append(rendered)
    result = ", ".join(upgraded)
    if result.startswith("^") and not value.startswith("^"):
        result = result[1:]
    return result


def _manifest_dependency_tables(document):
    result = {}
    tables = [("package", document), ("workspace", document.get("workspace", {}))]
    tables.extend(("target:" + key, table)
                  for key, table in document.get("target", {}).items())
    for prefix, table in tables:
        for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
            result[(prefix, kind)] = table.get(kind, {})
    return result


def preparation_dependency_versions(before, after, afterversions):
    """Bind each changed local requirement to its source package's next version."""
    documents = {path: tomllib.loads(raw.decode("utf-8")) for path, raw in after.items()}
    for path, raw in before.items():
        old_tables = _manifest_dependency_tables(tomllib.loads(raw.decode("utf-8")))
        new_tables = _manifest_dependency_tables(documents[path])
        for key, table in old_tables.items():
            for alias, old in table.items():
                new = new_tables[key][alias]
                if not isinstance(old, dict) or not isinstance(new, dict):
                    continue
                if old.get("version") == new.get("version"):
                    continue
                local = old.get("path")
                require(isinstance(local, str) and not local.startswith("/") and
                        "\\" not in local, "proposal_dependency_path")
                base = posixpath.dirname(path)
                manifest = posixpath.normpath(posixpath.join(base, local, "Cargo.toml"))
                require(manifest in documents, "proposal_dependency_source")
                name = documents[manifest].get("package", {}).get("name")
                require(name == old.get("package", alias) and name in afterversions,
                        "proposal_dependency_package")
                require(new.get("version") == _upgrade_requirement(old.get("version"),
                                                                   afterversions[name]),
                        "proposal_dependency_version")


def _lock_shape(document, selected=None):
    value = copy.deepcopy(document)
    local = {package.get("name") for package in value.get("package", [])
             if "source" not in package and (selected is None or package.get("name") in selected)}
    for package in value.get("package", []):
        if "source" in package:
            continue
        if package.get("name") in local:
            _require_version(package["version"])
            package["version"] = "<version>"
        # Release-plz regenerates local package references in dependency entries.
        for index, dependency in enumerate(package.get("dependencies", [])):
            parts = dependency.split(" ")
            if len(parts) == 2 and parts[0] in local and VERSION.fullmatch(parts[1]):
                parts[1] = "<version>"
            package["dependencies"][index] = " ".join(parts)
    return value


def _same_toml(left, right):
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return set(left) == set(right) and all(_same_toml(left[key], right[key]) for key in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(_same_toml(a, b) for a, b in zip(left, right))
    return repr(left) == repr(right)


def preparation_bytes(before, after, path, selected=None):
    """Require parsable bounded changes that cannot alter executable source/config."""
    require(isinstance(after, bytes) and len(after) <= 2 * 1024 * 1024,
            "proposal_file_size")
    after.decode("utf-8")
    if path.endswith("CHANGELOG.md"):
        require(b"\x00" not in after, "proposal_changelog_nul")
        return
    require(before is not None, "proposal_new_manifest_or_lock")
    old = tomllib.loads(before.decode("utf-8"))
    new = tomllib.loads(after.decode("utf-8"))
    shape = _manifest_shape if path.endswith("Cargo.toml") else _lock_shape
    require(_same_toml(shape(old, selected), shape(new, selected)), "proposal_nonversion_edit")

"""Unregistered pure Forge intent; only authenticated source supplies notes.

Official preexecution Cargo metadata and compiler literals supply identity and
release membership. The owned materialization root is lexical translation data,
never a filesystem reader or source authority. No candidate descriptor is read.
"""
from pathlib import PurePosixPath
import re
import tomllib


_INTENT_NUMBER = r"(?:0|[1-9][0-9]*)"
_INTENT_PRE = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
_INTENT_VERSION = re.compile(
    rf"{_INTENT_NUMBER}\.{_INTENT_NUMBER}\.{_INTENT_NUMBER}"
    rf"(?:-{_INTENT_PRE}(?:\.{_INTENT_PRE})*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?")


def _intent_absolute(value):
    require(type(value) is str and value.startswith("/") and "\\" not in value and
            all(ord(char) >= 32 and not 127 <= ord(char) <= 159 for char in value),
            "source_intent_absolute_path")
    path = PurePosixPath(value)
    require(str(path) == value and path.anchor == "/" and
            all(part not in (".", "..") for part in path.parts),
            "source_intent_path_alias")
    return path


def _intent_relative(value, root):
    path = _intent_absolute(value)
    require(path.is_relative_to(root), "source_intent_path_outside_root")
    relative = path.relative_to(root)
    require(relative.parts and all(part.lower() != ".git" for part in relative.parts),
            "source_intent_source_path")
    return relative


def _intent_version(value):
    require(type(value) is str and _INTENT_VERSION.fullmatch(value),
            "source_intent_version")
    numbers = value.partition("-")[0].partition("+")[0].split(".")
    require(all(len(number) <= 20 and int(number) <= 18446744073709551615
                for number in numbers), "source_intent_version_overflow")
    return value


def _intent_workspace(metadata, root):
    require(type(metadata) is dict and type(metadata.get("packages")) is list and
            type(metadata.get("workspace_members")) is list,
            "source_intent_metadata")
    workspace_root = _intent_absolute(metadata.get("workspace_root"))
    require(workspace_root.is_relative_to(root), "source_intent_workspace_root")
    packages, names = {}, set()
    for package in metadata["packages"]:
        require(type(package) is dict and type(package.get("id")) is str and
                package["id"] and package["id"] not in packages,
                "source_intent_package_id")
        name = package.get("name")
        require(type(name) is str and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]{0,63}", name),
                "source_intent_package_name")
        _intent_version(package.get("version"))
        manifest = _intent_relative(package.get("manifest_path"), root)
        require(manifest.name == "Cargo.toml", "source_intent_manifest")
        packages[package["id"]] = package, manifest
    members = metadata["workspace_members"]
    require(members and all(type(member) is str for member in members) and
            len(set(members)) == len(members) and set(members) <= set(packages),
            "source_intent_workspace_members")
    workspace = {}
    for member in members:
        package, manifest = packages[member]
        name = package["name"]
        require(name not in names, "source_intent_duplicate_name")
        names.add(name)
        workspace[name] = package, manifest
    return workspace


def _intent_enabled(config_text, workspace):
    require(type(config_text) is str, "source_intent_compiled_config")
    config = tomllib.loads(config_text)
    require(set(config) == {"workspace", "package"}, "source_intent_config_fields")
    defaults, overrides = config["workspace"], config["package"]
    switches = {"release": False, "release_always": False, "semver_check": True,
                "publish_no_verify": False, "publish_allow_dirty": False}
    require(type(defaults) is dict and set(defaults) == set(switches) | {"git_tag_name"} and
            all(defaults[key] is value for key, value in switches.items()) and
            type(overrides) is list, "source_intent_release_membership")
    pattern = defaults["git_tag_name"]
    require(type(pattern) is str and "{{ package }}" in pattern and
            "{{ version }}" in pattern and
            not any(ord(char) < 32 or ord(char) == 127 for char in pattern),
            "source_intent_tag_pattern")
    remainder = pattern.replace("{{ package }}", "").replace("{{ version }}", "")
    require("{" not in remainder and "}" not in remainder, "source_intent_tag_template")
    enabled, seen = set(), set()
    for override in overrides:
        fields = {"name", "release", "publish", "git_only"}
        require(type(override) is dict and set(override) in (fields, fields | {"version_group"}) and
                type(override.get("name")) is str and
                override["name"] in workspace and override["name"] not in seen and
                type(override.get("release")) is bool and override.get("publish") is True and
                override.get("git_only") is False,
                "source_intent_release_override")
        if "version_group" in override:
            group = override["version_group"]
            require(type(group) is str and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", group),
                    "source_intent_version_group")
        name = override["name"]
        seen.add(name)
        if override["release"]:
            enabled.add(name)
    require(enabled, "source_intent_no_enabled_members")
    return enabled, pattern


def forge_release_intent(full_metadata, source_cap, effective_policy, source_root):
    """Derive selected five-field descriptors using the full enabled workspace."""
    require(type(effective_policy) is dict and set(effective_policy) == {
        "approved", "release_config"}, "source_intent_effective_policy")
    approved = effective_policy["approved"]
    require(type(approved) is dict and type(approved.get("packages")) is dict and
            approved["packages"] and type(approved.get("tags")) is dict and
            set(approved["packages"]) == set(approved["tags"]),
            "source_intent_approved_packages")
    root = _intent_absolute(source_root)
    require(root != PurePosixPath("/"), "source_intent_source_root")
    workspace = _intent_workspace(full_metadata, root)
    enabled, pattern = _intent_enabled(effective_policy["release_config"], workspace)
    require(set(approved["packages"]) <= enabled, "source_intent_selected_members")
    selected = []
    for name, version in approved["packages"].items():
        _intent_version(version)
        package, manifest = workspace[name]
        tag = approved["tags"][name]
        expected_tag = pattern.replace("{{ package }}", name).replace("{{ version }}", version)
        require(package["version"] == version and type(tag) is str and tag and
                tag == expected_tag and
                len(tag) <= 256 and not any(ord(char) < 32 or ord(char) == 127 for char in tag),
                "source_intent_selected_identity")
        selected.append((name, version, tag, str(manifest.parent / "CHANGELOG.md")))
    result = {}
    for name, version, tag, changelog in selected:
        raw = authenticated_source_file(source_cap, changelog, limit=1024 * 1024)
        require(type(raw) is bytes and len(raw) <= 1024 * 1024,
                "source_intent_changelog_bytes")
        body = preparation_notes(raw, version)
        release_name = f"{name}-v{version}" if len(enabled) > 1 else f"v{version}"
        require(type(body) is str and len(body) <= 65536 and len(release_name) <= 256,
                "source_intent_descriptor_size")
        result[name] = {"tag_name": tag, "body": body, "name": release_name,
                        "draft": False, "prerelease": "-" in version.partition("+")[0]}
    return result

"""Read-only verification of exact annotated forge releases."""

import re
from urllib.parse import quote


DESCRIPTOR_FIELDS = {"tag_name", "body", "name", "draft", "prerelease"}
SHA = r"[0-9a-f]{40}"


def _descriptor(approved, name, package):
    require(isinstance(package, dict), "forge_release_descriptor")
    descriptor = package.get("forge_release")
    require(isinstance(descriptor, dict) and set(descriptor) == DESCRIPTOR_FIELDS,
            "forge_release_descriptor")
    tag = approved["tags"].get(name)
    version = approved["packages"].get(name)
    prerelease = isinstance(version, str) and "-" in version.partition("+")[0]
    require(isinstance(tag, str) and descriptor["tag_name"] == tag and
            isinstance(descriptor["body"], str) and len(descriptor["body"]) <= 65536 and
            isinstance(descriptor["name"], str) and 0 < len(descriptor["name"]) <= 256 and
            descriptor["draft"] is False and type(descriptor["prerelease"]) is bool and
            descriptor["prerelease"] is prerelease,
            "forge_release_descriptor")
    return descriptor


def _tag_message(name, version):
    return f"chore: Release package {name} version {version}"


def _tag_details(approved, tag, tag_object, expected_sha=None):
    require(isinstance(tag_object, dict) and set(tag_object) >= {
        "sha", "tag", "message", "object",
    } and isinstance(tag_object.get("sha"), str) and
            re.fullmatch(SHA, tag_object["sha"]) and
            tag_object.get("tag") == tag and
            isinstance(tag_object.get("message"), str) and
            isinstance(tag_object.get("object"), dict) and
            tag_object["object"].get("type") == "commit" and
            tag_object["object"].get("sha") == approved["source_sha"] and
            (expected_sha is None or tag_object["sha"] == expected_sha),
            "forge_tag_proof")
    return {
        "name": tag,
        "object_sha": tag_object["sha"],
        "source_sha": approved["source_sha"],
        "message": tag_object["message"],
    }


def _tag_observation(approved, name, version):
    repository = approved["repository"]
    tag = approved["tags"][name]
    ref = read_tag_ref(repository, tag)
    if ref is None:
        return None
    require(isinstance(ref, dict) and ref.get("ref") == "refs/tags/" + tag and
            isinstance(ref.get("object"), dict) and ref["object"].get("type") == "tag" and
            isinstance(ref["object"].get("sha"), str) and
            re.fullmatch(SHA, ref["object"]["sha"]), "forge_tag_ref")
    tag_sha = ref["object"]["sha"]
    tag_object = read_tag_object(repository, tag_sha)
    details = _tag_details(approved, tag, tag_object, tag_sha)
    require(tag_object["message"] == _tag_message(name, version), "forge_tag_message")
    return details


def _release_url(repository, tag):
    return f"https://github.com/{repository}/releases/tag/{quote(tag, safe='/')}"


def _release_details(repository, descriptor, response):
    require(isinstance(response, dict) and
            all(response.get(key) == descriptor[key]
                for key in ("tag_name", "body", "name")) and
            response.get("draft") is False and
            type(response.get("prerelease")) is bool and
            response["prerelease"] is descriptor["prerelease"] and
            response.get("html_url") == _release_url(repository, descriptor["tag_name"]),
            "forge_release_proof")
    return {
        "tag_name": descriptor["tag_name"],
        "body": descriptor["body"],
        "name": descriptor["name"],
        "draft": False,
        "prerelease": descriptor["prerelease"],
        "html_url": response["html_url"],
    }


def verify_forge_package(approved, name, package):
    """Read and verify one published tag and its exact frozen release."""
    descriptor = _descriptor(approved, name, {"forge_release": package})
    version = approved["packages"][name]
    tag = _tag_observation(approved, name, version)
    require(tag is not None, "forge_tag_missing")
    release = read_release(approved["repository"], descriptor["tag_name"])
    require(release is not None, "forge_release_missing")
    details = _release_details(approved["repository"], descriptor, release)
    return {
        "status": "verified",
        "tag": tag["name"],
        "target": tag["object_sha"],
        "release_url": details["html_url"],
    }

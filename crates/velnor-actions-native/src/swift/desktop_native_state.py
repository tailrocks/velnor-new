"""Read-only GitHub release inventory. Missing releases differ from fetch failures."""
import json
import os
import re
from urllib.error import HTTPError
from urllib.request import HTTPRedirectHandler, Request, build_opener

from desktop_native_core import version_build
from desktop_native_sign import archive_path


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, url):
        raise RuntimeError("release inventory redirects are forbidden")


def urlopen(request, timeout):
    return build_opener(NoRedirect()).open(request, timeout=timeout)


def validate_repository(repository):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*", repository):
        raise RuntimeError("repository must be owner/name")
    return repository


def fetch_release(repository, version):
    url = f"https://api.github.com/repos/{repository}/releases/tags/v{version}"
    headers = {"Accept": "application/vnd.github+json", "User-Agent": "velnor-native-release-state",
               "X-GitHub-Api-Version": "2022-11-28"}
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = "Bearer " + token
    request = Request(url, headers=headers, method="GET")
    try:
        with urlopen(request, timeout=30) as response:
            if response.status != 200:
                raise RuntimeError("release inventory returned unexpected HTTP status")
            payload = json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return None
        raise RuntimeError(f"release inventory failed: HTTP {error.code}") from error
    if not isinstance(payload, dict) or not isinstance(payload.get("assets"), list):
        raise RuntimeError("release inventory returned malformed JSON")
    return payload


def compute_state(asset, release):
    if release is None:
        names = set()
    else:
        names = set()
        for entry in release["assets"]:
            if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
                raise RuntimeError("release inventory contains malformed asset")
            names.add(entry["name"])
    required = {asset, asset + ".sha256", asset + ".bundle", asset + ".sbom.json"}
    complete = release is not None and required.issubset(names)
    return {"release_exists": release is not None,
            "app_file_assets_complete": complete, "complete": complete, "asset": asset}


def release_state(profile, version, repository, homebrew_tap=None):
    version_build(version, "1")
    validate_repository(repository)
    if homebrew_tap is not None:
        raise RuntimeError("Homebrew state requires an explicit native package profile")
    asset = archive_path(profile, version).name
    state = compute_state(asset, fetch_release(repository, version))
    for key, value in state.items():
        print(f"{key}={str(value).lower() if isinstance(value, bool) else value}")
    return state

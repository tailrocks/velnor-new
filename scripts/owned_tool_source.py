"""Admit measured, committed owned source without executing source code."""

import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tarfile
import unicodedata
import urllib.request


MAX_DOWNLOAD = 128 * 1024 * 1024
MAX_BOOTSTRAP_DOWNLOAD = 192 * 1024 * 1024
MAX_BOOTSTRAP_EXECUTABLE_BYTES = 192 * 1024 * 1024
MAX_SOURCE = 512 * 1024 * 1024
MAX_FILES = 30000
HOSTS = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
         "aarch64-apple-darwin"]
FIELDS = {"tool", "version", "source_commit", "source_tree", "upstream_base_commit",
          "archive_url", "archive_sha256", "receipt_url", "receipt_sha256",
          "patch_url", "patch_sha256", "lockfile_sha256", "license_files"}
OWNED_VERSION_PATTERN = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-(?:owned-[a-z0-9]+(?:-[a-z0-9]+)*|velnor\.[1-9][0-9]*)"
PORTABLE_LICENSE_PATTERN = r"[A-Za-z0-9._/-]+"
BASES = {"mise": ("jdx/mise", "8369bc78dc4761660337a2518b1445447438159d"),
         "mbx": ("jdx/mr-boxington", "44f50c329a83dedd012e1e49cf9f265f79226a83")}
BOOTSTRAP = ["rust@1.99.1"]
PREFIX = ["--no-config", "--no-env", "--no-hooks", "exec", *BOOTSTRAP, "--"]
BUILD = {
    "mise": ["<verified-bootstrap-mbx>", "build", "--release", "--locked", "--package", "mise",
             "--bin", "mise", "--no-default-features", "--features",
             "native-tls,vfox/vendored-lua,owned-cargo-wrapper"],
    "mbx": ["<verified-bootstrap-mbx>", "build", "--release", "--locked", "--package", "mbx",
            "--bin", "mbx", "--features", "owned-cache-transport"],
}
ABI = {"mise": "mise-owned-cargo-wrapper-v1", "mbx": None}

OFFICIAL_MISE_VERSION = '2026.11.3'
OFFICIAL_MISE_SOURCE_COMMIT = 'e6ae670b979f27d04e6c2ca77a7e88d300ee2add'
OFFICIAL_MISE_SOURCE_TREE = 'ddcbc2699d55c7e0d8f76a716b4f08ed9e025bb7'
OFFICIAL_MISE_BASE = 'https://github.com/jdx/mise/releases/download/v2026.11.3/mise-v2026.11.3-'
OFFICIAL_MISE_PINS = {
    "linux-x64": ("standalone", "c2f7e814cb39f60b7f67a0ff36cb805e94128bf2dff09d175309d0f095b8cc3b",
                  "c2f7e814cb39f60b7f67a0ff36cb805e94128bf2dff09d175309d0f095b8cc3b"),
    "linux-arm64.tar.gz": ("tar.gz", "13547a80d1dc2d5f81bb183023732f4981486e894d060e30993cc4364b4af51d",
                           "a9a2a66bbbe87ea9cc849e1f006606ae2b34f3d3ba56a54d5cd5d64b6557d164"),
    "macos-arm64.tar.gz": ("tar.gz", "de9a54330fee4edede51a3cf6196bbf522f01c5e3fd981e0bebea33f561a0431",
                           "6d39119ab914c54ab865204dee9337c71b83a7ed1e730f663c2fc3453e38fe9c"),
}

OFFICIAL_MISE_HOSTS = {"x86_64-unknown-linux-gnu": "linux-x64",
                      "aarch64-unknown-linux-gnu": "linux-arm64.tar.gz",
                      "aarch64-apple-darwin": "macos-arm64.tar.gz"}

OFFICIAL_MBX_VERSION = "1.29.2"
OFFICIAL_MBX_SOURCE_COMMIT = "7fa83b28d239bd39d1079127ffd2f8b0f92df00a"
OFFICIAL_MBX_SOURCE_TREE = "f0bcb9892fd7597d0acd741fc76f6ea92ffad501"
OFFICIAL_MBX_ASSETS = {
    "x86_64-unknown-linux-gnu": {
        "url": "https://github.com/jdx/mr-boxington/releases/download/v1.29.2/mbx-x86_64-unknown-linux-gnu.tar.gz",
        "archive_sha256": "73e135d3e9e8b8ddc938b42407179a239917b7f02d3fa5f601f6783e5c0d36d6",
        "binary_sha256": "03337a94062a76bf08df445959a3b732416c3931752602edb8f36b12474a9400",
        "format": "tar.gz", "binary_member": "mbx", "version": OFFICIAL_MBX_VERSION,
        "source_repository": "https://github.com/jdx/mr-boxington",
        "source_commit": OFFICIAL_MBX_SOURCE_COMMIT,
        "source_tree": OFFICIAL_MBX_SOURCE_TREE},
    "aarch64-unknown-linux-gnu": {
        "url": "https://github.com/jdx/mr-boxington/releases/download/v1.29.2/mbx-aarch64-unknown-linux-gnu.tar.gz",
        "archive_sha256": "1b6fbaebe0ea2c9b565c862a2b46e4756394ad530ce95b07333743104750a6fb",
        "binary_sha256": "31c0dd83d4c54d942b6e3d04edcf7064bb0536f6ce5a64806c130f17377d478a",
        "format": "tar.gz", "binary_member": "mbx", "version": OFFICIAL_MBX_VERSION,
        "source_repository": "https://github.com/jdx/mr-boxington",
        "source_commit": OFFICIAL_MBX_SOURCE_COMMIT,
        "source_tree": OFFICIAL_MBX_SOURCE_TREE},
    "aarch64-apple-darwin": {
        "url": "https://github.com/jdx/mr-boxington/releases/download/v1.29.2/mbx-aarch64-apple-darwin.tar.gz",
        "archive_sha256": "621ce235d8f241c5a3728ca03d34b9864f466f4e8b3307d8a6d7101a24a71a60",
        "binary_sha256": "c3f14e1e8564f0aae89e5157f5c0d06cf0e38f17072f170bd9364e2b7ef3781e",
        "format": "tar.gz", "binary_member": "mbx", "version": OFFICIAL_MBX_VERSION,
        "source_repository": "https://github.com/jdx/mr-boxington",
        "source_commit": OFFICIAL_MBX_SOURCE_COMMIT,
        "source_tree": OFFICIAL_MBX_SOURCE_TREE},
}
BOOTSTRAP_ASSET_FIELDS = {"url", "archive_sha256", "binary_sha256", "format", "binary_member",
                          "version", "source_repository", "source_commit", "source_tree"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def recipe(tool):
    return {"schema": 1, "bootstrap_mise": {"version": OFFICIAL_MISE_VERSION, "assets": official_assets("mise")},
            "bootstrap_mbx": {"version": OFFICIAL_MBX_VERSION, "assets": official_assets("mbx")},
            "argv": PREFIX + BUILD[tool], "behavior_abi": ABI[tool],
            "source_admission": "git-tree-lock-license-base-patch-v1"}


def official_assets(tool):
    if tool == "mbx":
        return OFFICIAL_MBX_ASSETS
    if tool != "mise":
        raise ValueError("unsupported bootstrap tool")
    assets = {}
    for host, name in OFFICIAL_MISE_HOSTS.items():
        archive_format, archive_sha, binary_sha = OFFICIAL_MISE_PINS[name]
        assets[host] = {"url": OFFICIAL_MISE_BASE + name, "format": archive_format,
                        "archive_sha256": archive_sha, "binary_sha256": binary_sha,
                        "binary_member": "" if archive_format == "standalone" else "mise/bin/mise",
                        "version": OFFICIAL_MISE_VERSION, "source_repository": "https://github.com/jdx/mise",
                        "source_commit": OFFICIAL_MISE_SOURCE_COMMIT,
                        "source_tree": OFFICIAL_MISE_SOURCE_TREE}
    return assets


def build_bootstrap_descriptor(data, target):
    supplied = strict_json(data)
    if not isinstance(supplied, dict) or set(supplied) != {"mise", "mbx"} or target not in HOSTS:
        raise ValueError("unexpected native bootstrap descriptor")
    for tool in ("mise", "mbx"):
        asset = supplied[tool]
        if not isinstance(asset, dict) or set(asset) != BOOTSTRAP_ASSET_FIELDS:
            raise ValueError("unexpected official bootstrap asset fields")
        expected = official_assets(tool).get(target)
        if expected is None:
            raise ValueError("official " + tool + " bootstrap byte authority is absent")
        if asset != expected:
            raise ValueError("unapproved official " + tool + " bootstrap tuple")
    return supplied


def recipe_sha(tool):
    return digest(json.dumps(recipe(tool), sort_keys=True, separators=(",", ":")).encode())


def strict_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON key")
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique)


def source_path(name):
    if (not isinstance(name, str) or "\\" in name or "\0" in name
            or any(part in ("", ".", "..") or canonical(part) == ".git"
                   for part in name.split("/"))
            or PurePosixPath(name).is_absolute()):
        raise ValueError("invalid source path")
    return name


def canonical(name):
    return "".join(char for char in unicodedata.normalize("NFD", name)
                   if unicodedata.category(char) != "Cf").casefold()


def descriptor(data, workflow_commit):
    spec = strict_json(data)
    if not isinstance(spec, dict) or set(spec) != FIELDS:
        raise ValueError("unexpected source descriptor fields")
    if spec["tool"] not in BASES:
        raise ValueError("unsupported owned tool")
    if not isinstance(spec["version"], str) or not re.fullmatch(OWNED_VERSION_PATTERN, spec["version"]):
        raise ValueError("distinct owned release version required")
    for key in ("source_commit", "source_tree", "upstream_base_commit"):
        if (not isinstance(spec[key], str) or not re.fullmatch(r"[a-f0-9]{40}", spec[key])
                or spec[key] == "0" * 40):
            raise ValueError("invalid Git identity")
    if spec["source_commit"] in (workflow_commit, spec["upstream_base_commit"]):
        raise ValueError("source must differ from workflow and upstream commits")
    if spec["upstream_base_commit"] != BASES[spec["tool"]][1]:
        raise ValueError("unapproved upstream base")
    for key in ("archive_sha256", "receipt_sha256", "patch_sha256", "lockfile_sha256"):
        check_hash(spec[key])
    licenses = spec["license_files"]
    if not isinstance(licenses, dict) or not licenses or "LICENSE" not in licenses:
        raise ValueError("committed LICENSE required")
    for name, sha in licenses.items():
        if not re.fullmatch(PORTABLE_LICENSE_PATTERN, name):
            raise ValueError("nonportable committed license path")
        source_path(name)
        if any(unicodedata.category(char) == "Cc" for char in name):
            raise ValueError("control character in committed license path")
        check_hash(sha)
    url = spec["archive_url"]
    if not isinstance(url, str) or not re.fullmatch(
            r"https://github\.com/tailrocks/velnor-new/releases/download/[A-Za-z0-9][A-Za-z0-9._-]*/source\.tar", url):
        raise ValueError("source URL must name exact owned repository asset")
    directory = url.rsplit("/", 1)[0]
    tag = directory.rsplit("/", 1)[1]
    if tag == "latest" or ".." in tag:
        raise ValueError("unapproved source release tag")
    if spec["receipt_url"] != directory + "/source-receipt.json" or spec["patch_url"] != directory + "/base.patch":
        raise ValueError("source evidence must share exact release directory")
    return spec


def check_hash(value):
    if not isinstance(value, str) or not re.fullmatch(r"[a-f0-9]{64}", value) or value == "0" * 64:
        raise ValueError("invalid SHA256")


class HttpsRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        if not newurl.startswith("https://"):
            raise ValueError("insecure download redirect")
        return super().redirect_request(request, fp, code, msg, headers, newurl)


def fetch(url, sha, max_bytes=None):
    if max_bytes is None:
        max_bytes = MAX_DOWNLOAD
    with urllib.request.build_opener(HttpsRedirect()).open(url, timeout=120) as response:
        data = response.read(max_bytes + 1)
    if len(data) > max_bytes or digest(data) != sha:
        raise ValueError("source download size or SHA256 mismatch")
    return data


def validate_receipt(data, spec):
    receipt = strict_json(data)
    repository, base = BASES[spec["tool"]]
    expected = {
        "schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": spec["tool"],
        "upstream_repository": "https://github.com/" + repository,
        "upstream_base_commit": base, "source_commit": spec["source_commit"],
        "source_tree": spec["source_tree"],
        "source_archive": {"name": "source.tar", "sha256": spec["archive_sha256"]},
        "base_patch": {"name": "base.patch", "sha256": spec["patch_sha256"]},
        "lockfile": {"path": "Cargo.lock", "sha256": spec["lockfile_sha256"]},
        "license_files": spec["license_files"], "required_hosts": HOSTS,
        "publication": None, "behavioral_qualification": None,
        "signed_build_provenance": None,
    }
    if receipt != expected or type(receipt.get("schema")) is not int:
        raise ValueError("source receipt does not match approved source")
    return receipt


def archive_entries(data):
    archive = tarfile.open(fileobj=io.BytesIO(data), mode="r:")
    entries, links, total, names = {}, {}, 0, set()
    for entry in archive:
        if len(entries) >= MAX_FILES or not entry.name.startswith("source/"):
            raise ValueError("source archive member limit or prefix")
        name = source_path(entry.name[len("source/"):])
        metadata = entry.pax_headers
        if (canonical(name) in names or set(metadata) - {"path", "linkpath"}
                or metadata.get("path", entry.name) != entry.name
                or metadata.get("linkpath", entry.linkname) != entry.linkname
                or entry.uid != 0 or entry.gid != 0):
            raise ValueError("duplicate or unsupported archive metadata")
        names.add(canonical(name))
        if entry.isfile() and entry.mode in (0o644, 0o755):
            total += entry.size
            if entry.size < 0 or total > MAX_SOURCE:
                raise ValueError("source archive size bound exceeded")
        elif entry.issym() and entry.mode == 0o777 and entry.size == 0:
            if not entry.linkname or "\0" in entry.linkname or "\\" in entry.linkname:
                raise ValueError("invalid source symlink")
            links[name] = entry.linkname
        else:
            raise ValueError("unsupported source member type or mode")
        entries[name] = entry
    validate_links(entries, links)
    return archive, entries


def validate_links(entries, links):
    names = {canonical(name) for name in entries}
    for name in entries:
        parents = PurePosixPath(name).parents
        if any(canonical(str(parent)) in names for parent in parents if str(parent) != "."):
            raise ValueError("archive member used as parent directory")
    links = {canonical(name): target for name, target in links.items()}
    for name in links:
        pending, resolved, expansions = name.split("/"), [], 0
        while pending:
            component = pending.pop(0)
            if component in ("", "."):
                continue
            if component == "..":
                if not resolved:
                    raise ValueError("source symlink escapes root")
                resolved.pop()
                continue
            resolved.append(component)
            target = links.get(canonical("/".join(resolved)))
            if target is not None:
                expansions += 1
                if target.startswith("/") or expansions > 40:
                    raise ValueError("absolute or cyclic source symlink")
                resolved.pop()
                pending = target.split("/") + pending


def extract(data, destination):
    archive, entries = archive_entries(data)
    try:
        destination.mkdir(mode=0o700)
        for name, entry in entries.items():
            target = destination / name
            target.parent.mkdir(parents=True, exist_ok=True)
            if entry.issym():
                target.symlink_to(entry.linkname)
            else:
                with archive.extractfile(entry) as member:
                    target.write_bytes(member.read())
                target.chmod(entry.mode)
    finally:
        archive.close()


def git_runner(source, environment):
    env = {key: value for key, value in environment.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_ATTR_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0")
    def run(*arguments, data=None):
        try:
            return subprocess.run(["git", "--no-replace-objects", "-C", str(source),
                               "-c", "core.autocrlf=false", "-c", "core.filemode=true",
                               "-c", "core.symlinks=true", "-c", "core.hooksPath=" + os.devnull,
                               "-c", "protocol.file.allow=never", "-c", "protocol.ext.allow=never",
                               "-c", "fetch.fsckObjects=true", *arguments], env=env,
                              check=True, capture_output=True, input=data).stdout
        except subprocess.CalledProcessError as error:
            sys.stdout.write((error.stdout or b"").decode("utf-8", errors="replace"))
            sys.stderr.write((error.stderr or b"").decode("utf-8", errors="replace"))
            raise
    run("init", "--quiet", "--template=", "--object-format=sha1")
    (source / ".git/info").mkdir(exist_ok=True)
    (source / ".git/info/attributes").write_text(
        "* -text -filter -ident -working-tree-encoding\n", encoding="utf-8")
    return run


def git_tree(source, environment):
    run = git_runner(source, environment)
    run("add", "--force", "--all", "--", ".")
    return run("write-tree").decode().strip()


def verify_patch(spec, patch, source, environment):
    source.mkdir(mode=0o700)
    run = git_runner(source, environment)
    repository, base = BASES[spec["tool"]]
    run("fetch", "--quiet", "--depth=1", "--no-tags", "--no-recurse-submodules",
        "https://github.com/" + repository + ".git", base)
    if run("rev-parse", "FETCH_HEAD").decode().strip() != base:
        raise ValueError("upstream base commit mismatch")
    run("read-tree", "--reset", "-u", base)
    run("apply", "--index", "--binary", "--whitespace=nowarn", "-", data=patch)
    if run("write-tree").decode().strip() != spec["source_tree"]:
        raise ValueError("source patch does not reconstruct approved owned tree")


def admit(spec, source, environment):
    receipt = fetch(spec["receipt_url"], spec["receipt_sha256"])
    validate_receipt(receipt, spec)
    archive = fetch(spec["archive_url"], spec["archive_sha256"])
    patch = fetch(spec["patch_url"], spec["patch_sha256"])
    if not patch:
        raise ValueError("empty owned source patch")
    verify_patch(spec, patch, source.parent / "upstream-base", environment)
    extract(archive, source)
    if git_tree(source, environment) != spec["source_tree"]:
        raise ValueError("extracted Git tree mismatch")
    verify_committed_file(source, "Cargo.lock", spec["lockfile_sha256"])
    for name, sha in spec["license_files"].items():
        verify_committed_file(source, name, sha)
    return receipt


def verify_committed_file(source, name, sha):
    file = source / name
    if file.is_symlink() or not file.is_file() or digest(file.read_bytes()) != sha:
        raise ValueError("committed lock or license mismatch")

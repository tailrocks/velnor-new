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
         "mbx": ("jdx/mr-boxington", "5a50200abf386580642b87eca2995156316b56a7")}
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

OFFICIAL_MISE_VERSION = "2026.10.18"
OFFICIAL_MISE_SOURCE_COMMIT = "8369bc78dc4761660337a2518b1445447438159d"
OFFICIAL_MISE_SOURCE_TREE = "f6484ac76553c3dca10c7ea690d01e359dac098b"
OFFICIAL_MISE_BASE = "https://github.com/jdx/mise/releases/download/v2026.10.18/mise-v2026.10.18-"
OFFICIAL_MISE_PINS = {
    "linux-x64": ("standalone", "e97c38973336aa2ff350fea50f47b412b266fe263b102b7200ff1e9b118c6942",
                  "e97c38973336aa2ff350fea50f47b412b266fe263b102b7200ff1e9b118c6942"),
    "linux-arm64.tar.gz": ("tar.gz", "c5a49a47f8184f5649f3d8685b408bd72325a823f41cf618c60f3ffc303090a1",
                           "3a3c72376687ba21a588d0f5a2c88c76f483fb6cdece7b0fc43d5e3132b08a52"),
    "macos-arm64.tar.gz": ("tar.gz", "1b1cd6ad4a6176db5ddaf03bf71e131b26324af09cfcf14d7d7038bee60be85d",
                           "d40e2fcd96fed7c70b04af282c36534d87d0318bef89c16fa00c9f98e55ade88"),
}
OFFICIAL_MISE_HOSTS = {"x86_64-unknown-linux-gnu": "linux-x64",
                      "aarch64-unknown-linux-gnu": "linux-arm64.tar.gz",
                      "aarch64-apple-darwin": "macos-arm64.tar.gz"}

OFFICIAL_MBX_VERSION = "1.27.0"
OFFICIAL_MBX_SOURCE_COMMIT = "5a50200abf386580642b87eca2995156316b56a7"
OFFICIAL_MBX_SOURCE_TREE = "9d1061348a5724ab331264bf83eef9169878e94b"
OFFICIAL_MBX_ASSETS = {
    "x86_64-unknown-linux-gnu": {
        "url": "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-x86_64-unknown-linux-gnu.tar.gz",
        "archive_sha256": "f530d0870efcf15c845314ffccd049f53eaec9a35e9f42c0dec0e16b3135e31c",
        "binary_sha256": "d02159722e826f1ed0729b07abaa5767907b7f5f9d3c2658f832e33da42c1907",
        "format": "tar.gz", "binary_member": "mbx", "version": OFFICIAL_MBX_VERSION,
        "source_repository": "https://github.com/jdx/mr-boxington",
        "source_commit": OFFICIAL_MBX_SOURCE_COMMIT,
        "source_tree": OFFICIAL_MBX_SOURCE_TREE},
    "aarch64-unknown-linux-gnu": {
        "url": "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-aarch64-unknown-linux-gnu.tar.gz",
        "archive_sha256": "8a1aaf0a495e455d2ea09430c92c59169eef6a29a140c6b21a6d1581fe252f86",
        "binary_sha256": "1b464d5b790b146d92cdef0a923cf00cc6d2a3b3d5acd69083b2bd75d604f1d4",
        "format": "tar.gz", "binary_member": "mbx", "version": OFFICIAL_MBX_VERSION,
        "source_repository": "https://github.com/jdx/mr-boxington",
        "source_commit": OFFICIAL_MBX_SOURCE_COMMIT,
        "source_tree": OFFICIAL_MBX_SOURCE_TREE},
    "aarch64-apple-darwin": {
        "url": "https://github.com/jdx/mr-boxington/releases/download/v1.27.0/mbx-aarch64-apple-darwin.tar.gz",
        "archive_sha256": "b23a6f67c4fe8c5ef5c3f5b7a747de69d5c9ffe4d00c642eb6e835736cd8dc22",
        "binary_sha256": "13f3409d70ebb1b76992f8d28c57d2954b9bc6eccb01eeb738af64b23b386421",
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

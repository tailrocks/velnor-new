"""Source-only publication proof for closed reviewed source revisions.

Git object hashes and DCO trailers prove bytes and declared signoff, never a
cryptographic signature or behavioral qualification. No source code executes.
"""

import hashlib
import io
import json
import os
import re
import stat
import subprocess
import tarfile

from source_publication_records import ArchiveKind, reviewed_source_revision

REPO = "tailrocks/velnor-new"
HOSTS = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "aarch64-apple-darwin"]
STAGE_NAMES = {"source.tar", "base.patch", "source-receipt.json"}
LIMIT = 512 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def strict_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    return json.loads(data, object_pairs_hook=unique)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git_object(kind, data):
    return hashlib.sha1(kind.encode() + b" " + str(len(data)).encode() + b"\0" + data).hexdigest()


def read_regular(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_nlink == 1 and
                metadata.st_size <= LIMIT, "source evidence must be singly linked regular file")
        data = stream.read(LIMIT + 1)
    require(len(data) <= LIMIT, "source evidence exceeds size bound")
    return data


def git(repository, *arguments):
    environment = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_NO_LAZY_FETCH="1", GIT_TERMINAL_PROMPT="0", GIT_OPTIONAL_LOCKS="0")
    return subprocess.run(["git", "--no-replace-objects", "-C", str(repository),
        "-c", "core.hooksPath=" + os.devnull, *arguments], env=environment,
        capture_output=True, check=True).stdout


def source_identity(role, source_ref, source_commit, target):
    revision = reviewed_source_revision(role, source_commit, target)
    require(source_ref == revision.source_ref,
            "owned source ref differs from the reviewed revision")
    return revision


def committed_files(repository, commit):
    files = {}
    for entry in git(repository, "ls-tree", "-rtz", commit).split(b"\0"):
        if not entry:
            continue
        metadata, name = entry.split(b"\t", 1)
        mode, kind, object_id = metadata.split(b" ")
        name, object_id = name.decode(), object_id.decode()
        require(all(part not in ("", ".", "..") for part in name.split("/")) and
                "\\" not in name, "unsafe committed source path")
        if kind == b"tree":
            require(git_object("tree", git(repository, "cat-file", "tree", object_id)) == object_id,
                    "Git subtree object proof mismatch")
            continue
        require(kind == b"blob" and mode in (b"100644", b"100755", b"120000"),
                "source contains unsupported Git entry")
        files[name] = (mode, object_id)
    return files


def archive_proof(data, files):
    found, payloads = set(), {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as archive:
        for entry in archive:
            require(entry.name.startswith("source/"), "source archive prefix mismatch")
            name = entry.name[len("source/"):]
            require(name in files and name not in found and entry.size <= LIMIT,
                    "source archive member set mismatch")
            mode, object_id = files[name]
            if mode == b"120000":
                require(entry.issym() and entry.mode == 0o777, "source symlink mode mismatch")
                payload = entry.linkname.encode()
            else:
                require(entry.isfile() and entry.mode == (0o755 if mode == b"100755" else 0o644),
                        "source file mode mismatch")
                stream = archive.extractfile(entry)
                require(stream is not None, "source archive blob unavailable")
                payload = stream.read(LIMIT + 1)
            require(len(payload) <= LIMIT and git_object("blob", payload) == object_id,
                    "source archive Git blob proof mismatch")
            found.add(name)
            payloads[name] = payload
    require(found == set(files), "source archive omits committed source")
    return payloads


def receipt_proof(receipt, revision, payloads, files):
    role = revision.role.value
    lock = "package-lock.json" if role == "mbx-action" else "Cargo.lock"
    require(lock in payloads and files[lock][0] == b"100644", "committed regular lock required")
    licenses = {name: sha(data) for name, data in payloads.items() if files[name][0] == b"100644" and
                re.search(r"(^|/)(LICENSE|COPYING|NOTICE)([.\-]|$)", name)}
    require(licenses, "committed license proof missing")
    expected = {"schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": role,
        "upstream_repository": "https://github.com/" + revision.upstream_repository,
        "upstream_base_commit": revision.upstream_base_commit, "source_commit": revision.source_commit,
        "source_tree": revision.source_tree, "source_archive": {"name": "source.tar", "sha256": revision.source_archive_sha256},
        "base_patch": {"name": "base.patch", "sha256": revision.base_patch_sha256},
        "lockfile": {"path": lock, "sha256": sha(payloads[lock])}, "license_files": licenses,
        "required_hosts": [] if role == "mbx-action" else HOSTS, "publication": None,
        "behavioral_qualification": None, "signed_build_provenance": None}
    if role == "mbx-action":
        require("dist/index.js" in payloads and files["dist/index.js"][0] == b"100644",
                "committed action bundle proof missing")
        expected["action_bundle"] = {"path": "dist/index.js", "sha256": sha(payloads["dist/index.js"])}
    require(json.dumps(receipt, sort_keys=True) == json.dumps(expected, sort_keys=True),
            "stage receipt differs from committed source proof")


def raw_commit_proof(repository, revision):
    raw = git(repository, "cat-file", "commit", revision.source_commit)
    require(git_object("commit", raw) == revision.source_commit and
            sha(raw) == revision.raw_commit_sha256, "raw source commit proof mismatch")
    headers, message = raw.split(b"\n\n", 1)
    require(headers.splitlines()[0] == ("tree " + revision.source_tree).encode() and
            b"\ngpgsig " not in headers, "source commit tree or unsigned status mismatch")
    tree = git(repository, "cat-file", "tree", revision.source_tree)
    require(git_object("tree", tree) == revision.source_tree, "root Git tree proof mismatch")
    signoffs = re.findall(r"^Signed-off-by: (.+ <[^<>\n]+>)$", message.decode(), re.MULTILINE)
    require(signoffs and "Co-authored-by: Codex <codex@openai.com>" in message.decode(),
            "reviewed source DCO/coauthor trailers missing")
    git(repository, "merge-base", "--is-ancestor", revision.upstream_base_commit, revision.source_commit)
    return raw, signoffs


def prepare(stage, repository, role, source_ref, source_commit, target):
    revision = source_identity(role, source_ref, source_commit, target)
    require(not stage.is_symlink() and not repository.is_symlink(), "source directory symlink forbidden")
    stage, repository = stage.resolve(strict=True), repository.resolve(strict=True)
    require({path.name for path in stage.iterdir()} == STAGE_NAMES, "unexpected source stage files")
    assets = {name: read_regular(stage / name) for name in sorted(STAGE_NAMES)}
    require(sha(assets["source.tar"]) == revision.source_archive_sha256 and
            len(assets["source.tar"]) == revision.source_archive_size and
            sha(assets["base.patch"]) == revision.base_patch_sha256 and
            sha(assets["source-receipt.json"]) == revision.source_receipt_sha256,
            "reviewed source stage hash mismatch")
    raw, signoffs = raw_commit_proof(repository, revision)
    assets["source.commit"] = raw
    files = committed_files(repository, revision.source_commit)
    patch = git(repository, "diff", "--binary", "--full-index", "--no-ext-diff", "--no-textconv",
        "--no-renames", "--diff-algorithm=myers", "--src-prefix=a/", "--dst-prefix=b/",
        revision.upstream_base_commit, revision.source_commit, "--", ".")
    require(patch and patch == assets["base.patch"],
            "base patch does not match exact committed source")
    receipt = strict_json(assets["source-receipt.json"])
    if revision.archive_kind is ArchiveKind.GIT_TAR_UMASK_022_V1:
        from source_proof_capsule import validate_semver_receipt
        payloads = validate_semver_receipt(receipt, assets["source-receipt.json"], assets, revision, files)
    else:
        payloads = archive_proof(assets["source.tar"], files)
        receipt_proof(receipt, revision, payloads, files)
    manifest = {"schema": 1, "status": "SOURCE_ONLY", "tool": revision.role.value,
        "source_ref": source_ref, "source_commit": revision.source_commit, "source_tree": revision.source_tree,
        "upstream_base_commit": revision.upstream_base_commit, "tag": revision.tag,
        "tag_target": revision.tag_target, "repository": REPO, "source_receipt": receipt,
        "raw_commit": {"sha256": sha(raw), "git_object_sha1": revision.source_commit,
                       "dco_signoffs": signoffs, "cryptographic_signature": None},
        "assets": {name: {"sha256": sha(data), "size": len(data)} for name, data in assets.items()},
        "behavioral_qualification": None, "signed_build_provenance": None}
    assets["source-publication.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    return manifest, assets

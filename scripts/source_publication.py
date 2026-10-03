"""Source-only publication proof for two explicitly reviewed source revisions.

Git object hashes and DCO trailers prove bytes and declared signoff, never a
cryptographic signature or behavioral qualification. No source code executes.
"""

import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile

REPO = "tailrocks/velnor-new"
TARGET = "c57c700459bbe1549fe7eedcb7d8689585c38986"
HOSTS = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "aarch64-apple-darwin"]
APPROVED = {
    "mise": {"commit": "dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96",
        "tree": "d5eefb0470da013d4f524555df763f16a0faf22d",
        "base": "bc11f90c74eba23bf0d7350efb540e62fb7d9ffd", "upstream": "jdx/mise",
        "raw_commit_sha256": "fa982e90ef0857b2fb87eb4e4584197b3fc6371664ab47a404e85b7d4c4df773",
        "source.tar": "a2ed2eec09aecf7c964fbf408d6c50fae2ec62a8354f4afb9b5ddaa3812f5d92",
        "base.patch": "1faa8dd229d3f403695969fafb41cf5855e68d60fee04c456b851fb95a53637b",
        "source-receipt.json": "038587e5c20392736ebdfa25d8d10557b774cbca04f13a0c93b4581b83d7e670"},
    "mbx-action": {"commit": "c3cbe8e56ccb4727624df45022357f49d2953075",
        "tree": "57a9336f26b9ce4a31f5f914c17594ecaa9c1248",
        "base": "1687e54eb349cadf61fa38b5813a77875489e8e6", "upstream": "jdx/mr-boxington-action",
        "raw_commit_sha256": "496ca46524ad9ce2a55b01a454ac4925f897cd09139f08909291a0ac579c1dc0",
        "source.tar": "1aa5e5813cdaa08c8240693099c329d261bd93b69e97bef18e773d9e88dadcb1",
        "base.patch": "25310d9587c5cf5854014140f0fe97aa800303b21316a660fd67bb77f9c9a554",
        "source-receipt.json": "1798003a5f4287d774b6dfa3d54374efd3fce48a8c9543d29306d222458cae8a"},
}
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
    require(role in APPROVED, "unreviewed source role")
    approved = APPROVED[role]
    require(target == TARGET and source_commit == approved["commit"] and
            source_ref == "refs/heads/owned-source/" + role + "/" + source_commit,
            "source or protected publication target differs from reviewed tuple")
    return approved


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


def receipt_proof(receipt, role, approved, payloads, files):
    lock = "package-lock.json" if role == "mbx-action" else "Cargo.lock"
    require(lock in payloads and files[lock][0] == b"100644", "committed regular lock required")
    licenses = {name: sha(data) for name, data in payloads.items() if files[name][0] == b"100644" and
                re.search(r"(^|/)(LICENSE|COPYING|NOTICE)([.\-]|$)", name)}
    require(licenses, "committed license proof missing")
    expected = {"schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": role,
        "upstream_repository": "https://github.com/" + approved["upstream"],
        "upstream_base_commit": approved["base"], "source_commit": approved["commit"],
        "source_tree": approved["tree"], "source_archive": {"name": "source.tar", "sha256": approved["source.tar"]},
        "base_patch": {"name": "base.patch", "sha256": approved["base.patch"]},
        "lockfile": {"path": lock, "sha256": sha(payloads[lock])}, "license_files": licenses,
        "required_hosts": [] if role == "mbx-action" else HOSTS, "publication": None,
        "behavioral_qualification": None, "signed_build_provenance": None}
    if role == "mbx-action":
        require("dist/index.js" in payloads and files["dist/index.js"][0] == b"100644",
                "committed action bundle proof missing")
        expected["action_bundle"] = {"path": "dist/index.js", "sha256": sha(payloads["dist/index.js"])}
    require(json.dumps(receipt, sort_keys=True) == json.dumps(expected, sort_keys=True),
            "stage receipt differs from committed source proof")


def raw_commit_proof(repository, approved):
    raw = git(repository, "cat-file", "commit", approved["commit"])
    require(git_object("commit", raw) == approved["commit"] and
            sha(raw) == approved["raw_commit_sha256"], "raw source commit proof mismatch")
    headers, message = raw.split(b"\n\n", 1)
    require(headers.splitlines()[0] == ("tree " + approved["tree"]).encode() and
            b"\ngpgsig " not in headers, "source commit tree or unsigned status mismatch")
    tree = git(repository, "cat-file", "tree", approved["tree"])
    require(git_object("tree", tree) == approved["tree"], "root Git tree proof mismatch")
    signoffs = re.findall(r"^Signed-off-by: (.+ <[^<>\n]+>)$", message.decode(), re.MULTILINE)
    require(signoffs and "Co-authored-by: Codex <codex@openai.com>" in message.decode(),
            "reviewed source DCO/coauthor trailers missing")
    git(repository, "merge-base", "--is-ancestor", approved["base"], approved["commit"])
    return raw, signoffs


def prepare(stage, repository, role, source_ref, source_commit, target):
    approved = source_identity(role, source_ref, source_commit, target)
    require(not stage.is_symlink() and not repository.is_symlink(), "source directory symlink forbidden")
    stage, repository = stage.resolve(strict=True), repository.resolve(strict=True)
    require({path.name for path in stage.iterdir()} == STAGE_NAMES, "unexpected source stage files")
    assets = {name: read_regular(stage / name) for name in sorted(STAGE_NAMES)}
    for name, data in assets.items():
        require(sha(data) == approved[name], "reviewed source stage hash mismatch")
    raw, signoffs = raw_commit_proof(repository, approved)
    files = committed_files(repository, approved["commit"])
    payloads = archive_proof(assets["source.tar"], files)
    receipt = strict_json(assets["source-receipt.json"])
    receipt_proof(receipt, role, approved, payloads, files)
    patch = git(repository, "diff", "--binary", "--full-index", "--no-ext-diff", "--no-textconv",
        "--no-renames", "--diff-algorithm=myers", "--src-prefix=a/", "--dst-prefix=b/",
        approved["base"], approved["commit"], "--", ".")
    require(patch and patch == assets["base.patch"], "base patch does not match exact committed source")
    assets["source.commit"] = raw
    manifest = {"schema": 1, "status": "SOURCE_ONLY", "tool": role,
        "source_ref": source_ref, "source_commit": source_commit, "source_tree": approved["tree"],
        "upstream_base_commit": approved["base"], "tag": "owned-source-" + role + "-" + source_commit,
        "tag_target": target, "repository": REPO, "source_receipt": receipt,
        "raw_commit": {"sha256": sha(raw), "git_object_sha1": source_commit,
                       "dco_signoffs": signoffs, "cryptographic_signature": None},
        "assets": {name: {"sha256": sha(data), "size": len(data)} for name, data in assets.items()},
        "behavioral_qualification": None, "signed_build_provenance": None}
    assets["source-publication.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    return manifest, assets

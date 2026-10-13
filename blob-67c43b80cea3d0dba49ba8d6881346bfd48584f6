#!/usr/bin/env python3
"""Stage committed owned tool source for review; never publish or qualify it."""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile


UPSTREAM = {
    "mise": ("jdx/mise", "96cca90d3e55519a47cffa0cb99baa4c3"),
    "mbx": ("jdx/mr-boxington", "a0a44c61ca6aaa8da41d59deeebdfc46fc9d3313"),
    "mbx-action": ("jdx/mr-boxington-action", "1687e54eb349cadf61fa38b5813a77875489e8e6"),
}
HOSTS = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
         "aarch64-apple-darwin"]


def git(source, *arguments):
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith("GIT_")}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_OPTIONAL_LOCKS="0")
    return subprocess.run(["git", "--no-replace-objects", "-C", str(source),
                           *arguments], check=True, capture_output=True,
                          env=environment).stdout


def digest(data):
    return hashlib.sha256(data).hexdigest()


def source_archive(source, files):
    """Archive raw Git blobs; export-ignore/export-subst cannot alter source."""
    links = validate_symlinks(source, files)
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.PAX_FORMAT) as archive:
        for name, (mode, object_id) in sorted(files.items()):
            if (name.startswith("/") or any(part in ("", ".", "..")
                    for part in name.split("/")) or mode not in
                    (b"100644", b"100755", b"120000")):
                raise ValueError("unsupported source path or Git file mode")
            data = git(source, "cat-file", "blob", object_id)
            entry = tarfile.TarInfo("source/" + name)
            if mode == b"120000":
                entry.type = tarfile.SYMTYPE
                entry.linkname = links[name]
                entry.mode = 0o777
                archive.addfile(entry)
            else:
                entry.mode = 0o755 if mode == b"100755" else 0o644
                entry.size = len(data)
                archive.addfile(entry, io.BytesIO(data))
    return buffer.getvalue()


def validate_symlinks(source, files):
    links = {name: git(source, "cat-file", "blob", object_id).decode("utf-8")
             for name, (mode, object_id) in files.items() if mode == b"120000"}
    for name in links:
        pending, resolved, expanded = name.split("/"), [], 0
        while pending:
            component = pending.pop(0)
            if component in ("", "."):
                continue
            if component == "..":
                if not resolved:
                    raise ValueError("source symlink escapes archive root")
                resolved.pop()
                continue
            resolved.append(component)
            target = links.get("/".join(resolved))
            if target is not None:
                expanded += 1
                if target.startswith("/") or "\0" in target or expanded > 40:
                    raise ValueError("absolute or cyclic source symlink")
                resolved.pop()
                pending = target.split("/") + pending
    return links


def stage(source, tool, output):
    source = source.resolve(strict=True)
    root = Path(git(source, "rev-parse", "--show-toplevel").decode().strip()).resolve()
    if source != root:
        raise ValueError("source must be the repository root")
    repository, base = UPSTREAM[tool]
    commit = git(source, "rev-parse", "HEAD").decode().strip()
    if not re.fullmatch(r"[a-f0-9]{40}", commit) or commit == base:
        raise ValueError("owned source must be a distinct committed revision")
    if git(source, "status", "--porcelain", "--untracked-files=all"):
        raise ValueError("source is dirty; commit reviewed source before staging")
    git(source, "merge-base", "--is-ancestor", base, commit)
    tree = git(source, "rev-parse", commit + "^{tree}").decode().strip()
    entries = git(source, "ls-tree", "-rz", commit).split(b"\0")
    files = {}
    for entry in entries:
        if not entry:
            continue
        metadata, name = entry.split(b"\t", 1)
        mode, kind, object_id = metadata.split(b" ")
        if kind != b"blob":
            raise ValueError("gitlinks need an explicit source closure before staging")
        files[name.decode("utf-8")] = (mode, object_id.decode())
    lock = "package-lock.json" if tool == "mbx-action" else "Cargo.lock"
    if lock not in files or files[lock][0] != b"100644":
        raise ValueError("regular committed dependency lock is required")
    licenses = {name: digest(git(source, "cat-file", "blob", object_id))
                for name, (mode, object_id) in files.items()
                if mode == b"100644" and
                re.search(r"(^|/)(LICENSE|COPYING|NOTICE)([.\-]|$)", name)}
    if not licenses:
        raise ValueError("committed license files are required")
    archive = source_archive(source, files)
    patch = git(source, "diff", "--binary", "--full-index", "--no-ext-diff",
                "--no-textconv", "--no-renames", "--diff-algorithm=myers",
                "--src-prefix=a/", "--dst-prefix=b/", base, commit, "--", ".")
    if not patch:
        raise ValueError("owned revision has no source changes")
    receipt = {
        "schema": 1, "status": "STAGED_SOURCE_ONLY", "tool": tool,
        "upstream_repository": "https://github.com/" + repository,
        "upstream_base_commit": base, "source_commit": commit, "source_tree": tree,
        "source_archive": {"name": "source.tar", "sha256": digest(archive)},
        "base_patch": {"name": "base.patch", "sha256": digest(patch)},
        "lockfile": {"path": lock,
                     "sha256": digest(git(source, "cat-file", "blob", files[lock][1]))},
        "license_files": dict(sorted(licenses.items())),
        "required_hosts": [] if tool == "mbx-action" else HOSTS,
        "publication": None, "behavioral_qualification": None,
        "signed_build_provenance": None,
    }
    if tool == "mbx-action":
        if "dist/index.js" not in files or files["dist/index.js"][0] != b"100644":
            raise ValueError("committed executable action bundle is required")
        receipt["action_bundle"] = {
            "path": "dist/index.js",
            "sha256": digest(git(source, "cat-file", "blob", files["dist/index.js"][1])),
        }
    write_stage(output, archive, patch, receipt)
    return receipt


def write_stage(output, archive, patch, receipt):
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise ValueError("stage destination already exists")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".owned-source-", dir=output.parent) as temporary:
        directory = Path(temporary)
        for name, data in [("source.tar", archive), ("base.patch", patch),
                           ("source-receipt.json", (json.dumps(receipt, indent=2,
                            sort_keys=True) + "\n").encode())]:
            (directory / name).write_bytes(data)
            (directory / name).chmod(0o600)
        # mkdir is exclusive. Concurrent staging cannot replace an existing receipt.
        output.mkdir(mode=0o700)
        for file in directory.iterdir():
            file.rename(output / file.name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tool", choices=UPSTREAM)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    try:
        receipt = stage(arguments.source, arguments.tool, arguments.output)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit("source staging failed: " + str(error)) from error
    print(json.dumps({"status": receipt["status"], "source_commit":
                      receipt["source_commit"], "source_tree": receipt["source_tree"]}))


if __name__ == "__main__":
    main()

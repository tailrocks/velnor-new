# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess


def read_bounded(path, limit):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            raise ValueError("fixture input is not a bounded regular file")
        data = stream.read(limit + 1)
        if not data or len(data) > limit:
            raise ValueError("fixture input size invalid")
        return data


def write_evidence(destination, runner, candidate_root, supplied_intent, info, state, counts, packed):
    destination = Path(destination)
    if not destination.is_absolute() or destination.parent.resolve(strict=True) != Path(runner).resolve(strict=True):
        raise ValueError("fixture evidence destination is not the runner directory")
    candidate_sha = subprocess.check_output(
        ["git", "-c", f"safe.directory={candidate_root}", "-C", str(candidate_root), "rev-parse", "HEAD"],
        text=True,
    ).strip()
    if not re.fullmatch(r"[0-9a-f]{40}", candidate_sha):
        raise ValueError("fixture candidate commit invalid")
    supplied = read_bounded(supplied_intent, 1024 * 1024)
    effective = read_bounded(Path(info["workspace"]) / ".github/velnor-release-intent.toml", 1024 * 1024)
    package = info["package"]
    version = info["version"]
    tag = f"{package}-v{version}"
    reconciled = state["refs"]["refs/tags/velnor-receipt/12/1/reconciled"]
    started = state["refs"]["refs/tags/velnor-receipt/12/1/publication-started"]
    published_sha = hashlib.sha256(packed).hexdigest()
    if (state["uploads"] != 1 or state["published_sha256"] != published_sha
            or state["release"]["tag_name"] != tag
            or state["refs"][f"refs/tags/{tag}"] != info["source_sha"]
            or not re.fullmatch(r"[0-9a-f]{40}", started)
            or not re.fullmatch(r"[0-9a-f]{40}", reconciled)
            or any(counts[name] != 1 for name in ("oidc", "exchange", "revoke"))):
        raise ValueError("fixture publication observations do not match")
    evidence = {
        "schema": 1,
        "mode": "coordinate",
        "candidate_sha": candidate_sha,
        "repository": info["repository"],
        "source_sha": info["source_sha"],
        "package": package,
        "version": version,
        "supplied_intent_sha256": hashlib.sha256(supplied).hexdigest(),
        "effective_intent_sha256": hashlib.sha256(effective).hexdigest(),
        "published_sha256": published_sha,
        "tag_name": tag,
        "tag_target": state["refs"][f"refs/tags/{tag}"],
        "publication_started_ref": started,
        "reconciled_ref": reconciled,
        "uploads": state["uploads"],
        "oidc_requests": counts["oidc"],
        "token_exchanges": counts["exchange"],
        "token_revocations": counts["revoke"],
    }
    encoded = json.dumps(evidence, sort_keys=True, separators=(",", ":")).encode() + b"\n"
    if len(encoded) > 16384:
        raise ValueError("fixture evidence exceeds limit")
    descriptor = os.open(destination, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "wb") as output:
        output.write(encoded)
        output.flush()
        os.fsync(output.fileno())

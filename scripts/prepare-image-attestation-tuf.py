#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import re
import stat
import subprocess
import tempfile
import urllib.request

HOSTS = ("tuf-repo.github.com", "tuf-repo-cdn.sigstore.dev")


def bounded(path, maximum):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= maximum:
            raise ValueError("TUF snapshot file is not bounded regular data")
        data = stream.read(maximum + 1)
    if len(data) != metadata.st_size:
        raise ValueError("TUF snapshot file changed")
    return data


def validate(directory, identity):
    payload = bounded(directory / "manifest.json", 65536)
    if hashlib.sha256(payload).hexdigest() != identity:
        raise ValueError("TUF snapshot manifest mismatch")
    manifest = json.loads(payload)
    for host in manifest["hosts"].values():
        for entry in host["routes"].values():
            if not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]):
                raise ValueError("TUF snapshot object identity invalid")
            data = bounded(directory / "objects" / entry["sha256"], 1024 * 1024)
            if len(data) != entry["size"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
                raise ValueError("TUF snapshot object mismatch")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cache-root", type=pathlib.Path, required=True)
    parser.add_argument("--gh", default="gh")
    args = parser.parse_args()
    root = args.cache_root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".tuf-", dir=root) as temporary:
        stage = pathlib.Path(temporary)
        env = {"PATH": os.environ["PATH"], "HOME": str(stage / "home"), "XDG_CACHE_HOME": str(stage / "cache"), "GH_CONFIG_DIR": str(stage / "config")}
        subprocess.run([args.gh, "attestation", "trusted-root", "--verify-only"], env=env, check=True, timeout=180)
        manifest = {"schema": "velnor.signed-tuf-replay.v1", "hosts": {}}
        objects = stage / "objects"
        objects.mkdir()
        minimum = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(hours=24)
        for host in HOSTS:
            source = stage / "cache/gh/.sigstore/root" / host
            metadata = {name: json.loads((source / f"{name}.json").read_bytes()) for name in ("root", "timestamp", "snapshot", "targets")}
            routes = {}
            def add(route, path):
                data = path.read_bytes()
                if not 0 < len(data) <= 1024 * 1024:
                    raise ValueError("TUF response exceeds bound")
                digest = hashlib.sha256(data).hexdigest()
                (objects / digest).write_bytes(data)
                routes[route] = {"sha256": digest, "size": len(data)}
            for name in ("root", "timestamp", "snapshot", "targets"):
                expiry = datetime.datetime.fromisoformat(metadata[name]["signed"]["expires"].replace("Z", "+00:00"))
                if expiry < minimum:
                    raise ValueError(f"TUF {host}/{name} expires within 24 hours")
            initial = {"tuf-repo.github.com": 3, "tuf-repo-cdn.sigstore.dev": 14}[host]
            final_root = metadata["root"]["signed"]["version"]
            if not isinstance(final_root, int) or not initial <= final_root <= initial + 100:
                raise ValueError("TUF root rotation exceeds bound")
            for version in range(initial + 1, final_root + 1):
                route = f"/{version}.root.json"
                with urllib.request.urlopen(f"https://{host}{route}", timeout=30) as response:
                    data = response.read(1024 * 1024 + 1)
                path = stage / f"{host}-{version}.root.json"
                path.write_bytes(data)
                add(route, path)
            add("/timestamp.json", source / "timestamp.json")
            for name in ("snapshot", "targets"):
                add(f"/{metadata[name]['signed']['version']}.{name}.json", source / f"{name}.json")
            target = metadata["targets"]["signed"]["targets"]["trusted_root.json"]
            data = (source / "targets/trusted_root.json").read_bytes()
            if len(data) != target["length"] or hashlib.sha256(data).hexdigest() != target["hashes"]["sha256"]:
                raise ValueError("authenticated TUF target mismatch")
            add(f"/targets/{target['hashes']['sha256']}.trusted_root.json", source / "targets/trusted_root.json")
            manifest["hosts"][host] = {"routes": routes, "next_root": metadata["root"]["signed"]["version"] + 1, "expires": metadata["timestamp"]["signed"]["expires"]}
        payload = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
        identity = hashlib.sha256(payload).hexdigest()
        candidate = stage / "candidate"
        candidate.mkdir()
        (candidate / "manifest.json").write_bytes(payload)
        os.rename(objects, candidate / "objects")
        destination = root / identity
        if destination.exists():
            validate(destination, identity)
        else:
            os.rename(candidate, destination)
            validate(destination, identity)
        selection = root / ".selected"
        selection.write_text(identity)
        os.replace(selection, root / "selected")


if __name__ == "__main__":
    main()

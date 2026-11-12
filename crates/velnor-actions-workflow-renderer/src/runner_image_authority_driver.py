# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
import hashlib
import http.client
import os
from pathlib import Path
import re
import stat
import sys

import job_image_authority_registry as registry
from job_image_authority_transport import RegistryTransport
import runner_image_authority_identity as identity
import runner_image_authority_publish as publication

FILES = {
    "runner": ("velnor-runner-linux-amd64.tar", "runner-image-publication.json", "runner-image-provenance.json"),
    "dind": ("velnor-dind-linux-amd64.tar", "dind-image-publication.json", "dind-image-provenance.json"),
    "resource_probe": ("velnor-resource-probe-linux-amd64.tar", "resource-probe-image-publication.json", "resource-probe-image-provenance.json"),
}


def read_regular(path, limit):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= limit:
            raise publication.AuthorityError("Publication input is not a bounded regular file.")
        raw = stream.read(limit + 1)
        if len(raw) > limit:
            raise publication.AuthorityError("Publication input exceeds its size limit.")
        return raw


def positive(name):
    value = os.environ[name]
    if re.fullmatch("[1-9][0-9]{0,19}", value) is None or int(value) > 2**64 - 1:
        raise publication.AuthorityError("Publication run identity is invalid.")
    return int(value)


def authority_source():
    source = os.environ["GITHUB_SHA"]
    expected = {
        "GITHUB_REPOSITORY": identity.REPOSITORY,
        "GITHUB_REPOSITORY_ID": identity.REPOSITORY_ID,
        "GITHUB_REF": "refs/heads/main",
        "GITHUB_EVENT_NAME": "push",
        "GITHUB_WORKFLOW_REF": f"{identity.REPOSITORY}/.github/workflows/product-release-images.yml@refs/heads/main",
        "GITHUB_WORKFLOW_SHA": source,
    }
    if not identity._hex(source, 40) or any(os.environ.get(key) != value for key, value in expected.items()):
        raise publication.AuthorityError("Runner publication workflow authority is invalid.")
    return source


class CurrentSource:
    def __init__(self, token):
        self.token = token

    def __call__(self):
        connection = http.client.HTTPSConnection("api.github.com", timeout=30)
        try:
            connection.request("GET", f"/repos/{identity.REPOSITORY}/git/ref/heads/main", headers={
                "Authorization": f"Bearer {self.token}", "User-Agent": "velnor-fixed-runner-publisher",
                "Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28",
            })
            response = connection.getresponse()
            raw = response.read(65537)
            if response.status != 200 or len(raw) > 65536:
                raise publication.AuthorityError("Cannot authenticate current main source.")
            value = identity.parse_document(raw, 65536)
            if value.get("ref") != "refs/heads/main" or value.get("object", {}).get("type") != "commit":
                raise publication.AuthorityError("Current main response has invalid identity.")
            source = value["object"]["sha"]
            if not identity._hex(source, 40):
                raise publication.AuthorityError("Current main commit is invalid.")
            return source
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            raise publication.AuthorityError("Current main verification failed or timed out.") from error
        finally:
            connection.close()


def main():
    source = authority_source()
    root = Path(os.environ["RUNNER_TEMP"])
    assets = root / "runner-family-candidate"
    qualified = root / "runner-family-qualification"
    authenticated = root / "runner-family-authenticated"
    private = root / "runner-family-private"
    private.mkdir(mode=0o700)
    inputs = {role: {"archive": assets / names[0], "record": read_regular(assets / names[1], 1024**2),
                     "envelope": read_regular(assets / names[2], 8 * 1024**2)} for role, names in FILES.items()}
    transports = {role: RegistryTransport(30, registry.RegistryPolicy(profile)) for role, profile in publication.PROFILES.items()}
    manifest = publication.publish_family(
        inputs, read_regular(qualified / identity.RECEIPT_NAME, 1024**2),
        read_regular(authenticated / "qualification-verification.json", 2 * 1024**2),
        source, positive("PRODUCER_RUN_ID"), positive("PRODUCER_RUN_ATTEMPT"),
        positive("QUALIFIER_RUN_ID"), positive("QUALIFIER_RUN_ATTEMPT"), positive("QUALIFIER_JOB_ID"),
        read_regular(authenticated / "runner.lock", 1024**2), private,
        os.environ["GITHUB_ACTOR"], os.environ["GH_TOKEN"], transports, CurrentSource(os.environ["GH_TOKEN"]),
    )
    output = root / "runner-family-release" / "assets"
    output.mkdir(parents=True, mode=0o700)
    values = {"RUNNER_IMAGE_MANIFEST.json": manifest, "LICENSE": read_regular(authenticated / "LICENSE", 1024**2),
              "NOTICE": read_regular(authenticated / "NOTICE", 1024**2)}
    values["SHA256SUMS"] = "".join(f"{hashlib.sha256(raw).hexdigest()}  {name}\n" for name, raw in values.items()).encode()
    for name, raw in values.items():
        with (output / name).open("xb") as stream:
            stream.write(raw)


if __name__ == "__main__":
    try:
        main()
    except (KeyError, TypeError, AttributeError, ValueError, OSError, http.client.HTTPException) as error:
        print(f"Runner publication rejected: {type(error).__name__}", file=sys.stderr)
        raise SystemExit(1) from None

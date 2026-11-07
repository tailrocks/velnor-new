# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

workflow = sys.stdin.read()
match = re.search(r"run: bash -c 'set -o pipefail; printf %s ([A-Za-z0-9+/=]+) \| /usr/bin/base64 --decode \| /bin/bash'", workflow)
assert match, "rendered bootstrap missing"
script = base64.b64decode(match.group(1)).decode()
version = re.search(r"VELNOR_GENERATOR_VERSION: ([^\s]+)", workflow).group(1)
source = "b" * 40
name = f"velnor-actions-{version}-x86_64-unknown-linux-gnu"
repo = "https://github.com/tailrocks/velnor-new"
signer = f"{repo}/.github/workflows/product-release-generator.yml@refs/heads/main"
run = f"{repo}/actions/runs/123/attempts/1"
sha = lambda value: hashlib.sha256(value).hexdigest()
with tempfile.TemporaryDirectory() as root:
    root = Path(root)
    fixtures = root / "fixtures"
    fixtures.mkdir()
    jq = shutil.which("jq")
    assert jq, "jq is required for the bootstrap verification test"
    jq_wrapper = f"#!/bin/sh\nexec {shlex.quote(jq)} \"$@\"\n".encode()
    gh = b'''#!/bin/sh
set -eu
[ "${GH_TOKEN:-}" = fixture-token ]
case "$3" in
 */release-manifest.json) cat "$FIXTURES/manifest-result.json" ;;
 *) cat "$FIXTURES/binary-result.json" ;;
esac
'''
    curl = b'''#!/bin/sh
set -eu
[ "${GH_TOKEN+x}" != x ]
url= out=
while [ "$#" -gt 0 ]; do
 case "$1" in
  https://*) url=$1; shift ;;
  -o) out=$2; shift 2 ;;
  --connect-timeout|--max-time|--max-filesize|--max-redirs|--proto|--proto-redir) shift 2 ;;
  *) shift ;;
 esac
done
cp "$FIXTURES/${url##*/}" "$out"
'''
    tar = b'''#!/bin/sh
set -eu
while [ "$#" -gt 0 ]; do
 if [ "$1" = -C ]; then root=$2; break; fi
 shift
done
mkdir -p "$root/gh_2.103.3_linux_amd64/bin"
cp "$FIXTURES/fake-gh" "$root/gh_2.103.3_linux_amd64/bin/gh"
'''
    checksum = f'''#!{sys.executable}
import hashlib,sys
from pathlib import Path
if sys.argv[1:] == ['-c','-']:
 expected, path = sys.stdin.read().rstrip().split('  ', 1)
 sys.exit(0 if hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected else 1)
p=Path(sys.argv[1]); print(hashlib.sha256(p.read_bytes()).hexdigest(),p)
'''.encode()
    def executable(file, value):
        path = fixtures / file
        path.write_bytes(value)
        path.chmod(0o755)
        return str(path)
    fake_curl = executable("curl", curl)
    fake_tar = executable("tar", tar)
    fake_sum = executable("sha256sum", checksum)
    executable("fake-gh", gh)
    archive = b"fixture archive"
    (fixtures / "gh_2.103.3_linux_amd64.tar.gz").write_bytes(archive)
    (fixtures / "jq-linux-amd64").write_bytes(jq_wrapper)
    payload = b'#!/bin/sh\ntouch "$RUNNER_TEMP/generator-executed"\n'
    manifest = json.dumps({"schema":1,"repository":"tailrocks/velnor-new","version":version,"commit":source,"targets":[{"target":"x86_64-unknown-linux-gnu","artifact":f"{repo}/releases/download/v{version}/{name}","sha256":sha(payload)}]}, separators=(",", ":")).encode()
    (fixtures / "release-manifest.json").write_bytes(manifest)
    (fixtures / "release-manifest.json.intoto.jsonl").write_bytes(b"fixture")
    (fixtures / f"{name}.intoto.jsonl").write_bytes(b"fixture")
    (fixtures / name).write_bytes(payload)
    def attestation(subject, digest):
        cert = {"issuer":"https://token.actions.githubusercontent.com","subjectAlternativeName":signer,"buildSignerURI":signer,"buildSignerDigest":source,"sourceRepositoryURI":repo,"sourceRepositoryOwnerURI":"https://github.com/tailrocks","sourceRepositoryIdentifier":"1390620900","sourceRepositoryOwnerIdentifier":"78806509","sourceRepositoryDigest":source,"sourceRepositoryRef":"refs/heads/main","buildConfigURI":f"{repo}/.github/workflows/product-release.yml@refs/heads/main","buildConfigDigest":source,"buildTrigger":"workflow_dispatch","runnerEnvironment":"github-hosted","runInvocationURI":run}
        subjects = [{"name":subject,"digest":{"sha256":digest}}]
        if subject != "release-manifest.json":
            subjects += [{"name":subject+suffix,"digest":{"sha256":"a"*64}} for suffix in (".sha256", ".provenance.json")]
        build = {"buildType":"https://actions.github.io/buildtypes/workflow/v1","externalParameters":{"workflow":{"path":".github/workflows/product-release.yml","ref":"refs/heads/main","repository":repo}},"internalParameters":{"github":{"event_name":"workflow_dispatch","repository_id":"1390620900","repository_owner_id":"78806509","runner_environment":"github-hosted"}},"resolvedDependencies":[{"uri":f"git+{repo}@refs/heads/main","digest":{"gitCommit":source}}]}
        return [{"verificationResult":{"statement":{"predicateType":"https://slsa.dev/provenance/v1","subject":subjects,"predicate":{"buildDefinition":build,"runDetails":{"builder":{"id":signer},"metadata":{"invocationId":run}}}},"signature":{"certificate":cert}}}]
    manifest_attestation = attestation("release-manifest.json", sha(manifest))
    binary_attestation = attestation(name, sha(payload))
    def write_attestations():
        (fixtures / "manifest-result.json").write_text(json.dumps(manifest_attestation))
        (fixtures / "binary-result.json").write_text(json.dumps(binary_attestation))
    write_attestations()
    script = script.replace("/usr/bin/curl", shlex.quote(fake_curl)).replace("/usr/bin/tar", shlex.quote(fake_tar)).replace("/usr/bin/sha256sum", shlex.quote(fake_sum))
    script = script.replace("020468de7539ce70ef1bceaf7cde2e8c4f2ca6c3afb84642aabc5c97d9fc2a0d", sha(jq_wrapper)).replace("4c806978ffce1cec70853302a56d70ac6da4f19bae7adca4e1a8ab00633dd91d", sha(archive)).replace("b768184570b12536be4ac1528893f6340d1c6a7b40b56ad45bca9cd0e87ad34d", sha(gh))
    command = "set -o pipefail; printf %s " + base64.b64encode(script.encode()).decode() + " | /usr/bin/base64 --decode | /bin/bash"
    def execute(label, expected_source=source, expected_manifest=None):
        temp = root / label
        temp.mkdir()
        env = dict(os.environ, RUNNER_TEMP=str(temp), GH_TOKEN="fixture-token", FIXTURES=str(fixtures), VELNOR_GENERATOR_VERSION=version, VELNOR_GENERATOR_SOURCE_SHA=expected_source, VELNOR_GENERATOR_MANIFEST_SHA256=expected_manifest or sha(manifest))
        result = subprocess.run(["bash", "-c", command], env=env, capture_output=True, timeout=30)
        assert not (temp / "generator-executed").exists(), label + " executed the generator during bootstrap"
        return result, temp / "velnor" / "bin" / f"velnor-actions-{version}"
    result, staged = execute("good")
    assert result.returncode == 0 and staged.read_bytes() == payload, result.stderr
    (fixtures / name).write_bytes(payload + b"tamper")
    result, staged = execute("binary")
    assert result.returncode != 0 and not staged.exists()
    (fixtures / name).write_bytes(payload)
    for label, expected_source, expected_manifest in [("source", "c"*40, None), ("digest", source, "d"*64)]:
        result, staged = execute(label, expected_source, expected_manifest)
        assert result.returncode != 0 and not staged.exists()
    manifest_attestation[0]["verificationResult"]["signature"]["certificate"]["sourceRepositoryOwnerIdentifier"] = "0"
    write_attestations()
    result, staged = execute("identity")
    assert result.returncode != 0 and not staged.exists()
    manifest_attestation = attestation("release-manifest.json", sha(manifest))
    manifest_attestation[0]["verificationResult"]["signature"]["certificate"]["issuer"] = "https://invalid.example"
    write_attestations()
    result, staged = execute("issuer")
    assert result.returncode != 0 and not staged.exists()
    manifest_attestation = attestation("release-manifest.json", sha(manifest))
    binary_attestation[0]["verificationResult"]["signature"]["certificate"]["runInvocationURI"] = f"{repo}/actions/runs/456/attempts/1"
    binary_attestation[0]["verificationResult"]["statement"]["predicate"]["runDetails"]["metadata"]["invocationId"] = f"{repo}/actions/runs/456/attempts/1"
    write_attestations()
    result, staged = execute("run")
    assert result.returncode != 0 and not staged.exists()

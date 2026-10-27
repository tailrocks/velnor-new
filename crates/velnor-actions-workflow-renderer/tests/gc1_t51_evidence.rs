// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
pub(crate) struct EvidenceInputs {
    pub(crate) record: String,
    pub(crate) archive: Vec<u8>,
    pub(crate) ci_runs: String,
    pub(crate) authenticated_source: String,
    pub(crate) artifact_response: String,
    pub(crate) attestation: String,
}

pub(crate) fn evidence_inputs() -> EvidenceInputs {
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    EvidenceInputs {
        record: record_json(&parts, &archive),
        archive,
        ci_runs: ci_runs_json(),
        authenticated_source: authenticated_source_json(),
        artifact_response: artifact_response_json(),
        attestation: attestation_json(),
    }
}

/// Socket-level stub for the rendered `EVIDENCE_PROGRAM`: the program text runs
/// byte-identical, but `http.client` is emulated in-process so the full
/// publish-and-prove flow (streaming PUT, HEAD, chunked GET readback) executes
/// with no network. Blob PUT bodies arrive through repeated `send` calls, the
/// way the streaming transport emits them.
const EVIDENCE_WRAPPER: &str = r#"import os
import sys
import types
from pathlib import Path
from urllib.parse import parse_qsl

ROOT = Path(sys.argv[1])
SCRIPT = Path(sys.argv[2])
# Mirror the rendered invocation shape: argv[0] is the program, argv[1] is the
# materialized authority directory (already on sys.path), then the 8 inputs.
PROGRAM_ARGV = ["evidence_program_stub", sys.argv[1]] + sys.argv[3:]

sys.path.insert(0, str(ROOT))

import job_image_authority_publish as publish_module


class _StubError(Exception):
    pass


class _StubResponse:
    def __init__(self, status, headers, body):
        self.status = status
        self._headers = [(key.lower(), value) for key, value in headers]
        self._body = body
        self._offset = 0

    def getheaders(self):
        return list(self._headers)

    def getheader(self, name, default=None):
        for key, value in self._headers:
            if key == name.lower():
                return value
        return default

    def read(self, size=-1):
        if size is None or size < 0:
            size = len(self._body) - self._offset
        chunk = self._body[self._offset:self._offset + size]
        self._offset += len(chunk)
        return chunk

    def close(self):
        pass


class _StubConnection:
    store = {}
    puts = 0

    def __init__(self, host, timeout=None):
        self.host = host
        self._method = None
        self._target = None
        self._headers = {}
        self._sent = bytearray()

    def putrequest(self, method, target):
        self._method = method
        self._target = target

    def putheader(self, name, value):
        self._headers[name.lower()] = value

    def endheaders(self):
        pass

    def send(self, data):
        self._sent.extend(data)

    def getresponse(self):
        return _handle(self)

    def close(self):
        pass


def _blob_key(target):
    digest = target.split("?")[0].rsplit("/", 1)[-1]
    kind = "manifests" if "/manifests/" in target else "blobs"
    return f"{kind}:{digest}", digest


def _handle(connection):
    target = connection._target
    method = connection._method
    if method == "GET" and target == "/v2/":
        return _StubResponse(
            401,
            [("www-authenticate", 'Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:tailrocks/velnor-job:pull,push"')],
            b"",
        )
    if method == "GET" and target.startswith("/token"):
        if os.environ.get("VELNOR_STUB_OVERSIZE_TOKEN") == "1":
            return _StubResponse(200, [], b"x" * 70000)
        return _StubResponse(200, [], b'{"token": "evidence-stub-token-0123456789abcdef"}')
    if method == "HEAD":
        key, digest = _blob_key(target)
        if key in _StubConnection.store:
            data = _StubConnection.store[key]
            return _StubResponse(
                200,
                [("docker-content-digest", digest), ("content-length", str(len(data)))],
                b"",
            )
        return _StubResponse(404, [], b"")
    if method == "POST":
        return _StubResponse(
            202,
            [("location", "/v2/tailrocks/velnor-job/blobs/uploads/stub-uuid")],
            b"",
        )
    if method == "PUT":
        query = dict(parse_qsl(target.split("?", 1)[1] if "?" in target else ""))
        key, digest = _blob_key(target)
        if "/manifests/" in target:
            body = bytes(connection._sent)
            _StubConnection.store[key] = body
            _StubConnection.puts += 1
            return _StubResponse(201, [("docker-content-digest", digest)], b"")
        body = bytes(connection._sent)
        if len(body) != int(connection._headers.get("content-length", "-1")):
            raise _StubError("stubbed blob PUT length changed")
        _StubConnection.store[f"blobs:{query['digest']}"] = body
        _StubConnection.puts += 1
        return _StubResponse(201, [], b"")
    if method == "GET":
        key, digest = _blob_key(target)
        stored = _StubConnection.store[key]
        return _StubResponse(
            200,
            [("docker-content-digest", digest), ("content-length", str(len(stored)))],
            stored,
        )
    raise _StubError(f"unexpected stubbed call: {method} {target}")


stub_client = types.ModuleType("http.client")
stub_client.HTTPSConnection = _StubConnection
stub_client.HTTPException = _StubError
stub_http = types.ModuleType("http")
stub_http.client = stub_client
sys.modules["http"] = stub_http
sys.modules["http.client"] = stub_client

sys.argv = PROGRAM_ARGV
exec(compile(SCRIPT.read_text(), str(SCRIPT), "exec"))
"#;

pub(crate) fn run_evidence_program(
    fixture: &Fixture,
    inputs: &EvidenceInputs,
    record_text: &str,
    extra_env: &[(&str, &str)],
) -> Result<(i32, String, String), Box<dyn Error>> {
    let script = fixture.root.path().join("evidence_program.py");
    std::fs::write(
        &script,
        extract_program(EVIDENCE_OPEN, EVIDENCE_CLOSE, "evidence program")?,
    )?;
    let wrapper = fixture.root.path().join("evidence_wrapper.py");
    std::fs::write(&wrapper, EVIDENCE_WRAPPER)?;
    let record_path = write_bytes(
        fixture.root.path(),
        "evidence-candidate.json",
        record_text.as_bytes(),
    )?;
    let archive_path = write_bytes(fixture.root.path(), "evidence.oci.tar", &inputs.archive)?;
    let result_path = fixture
        .root
        .path()
        .join("publication.json")
        .to_str()
        .ok_or_else(|| fail("result path left its string shape"))?
        .to_owned();
    let ci_runs_path = write_bytes(
        fixture.root.path(),
        "ci-runs.json",
        inputs.ci_runs.as_bytes(),
    )?;
    let source_path = write_bytes(
        fixture.root.path(),
        "authenticated-source.json",
        inputs.authenticated_source.as_bytes(),
    )?;
    let artifact_path = write_bytes(
        fixture.root.path(),
        "artifact-response.json",
        inputs.artifact_response.as_bytes(),
    )?;
    let archive_attestation_path = write_bytes(
        fixture.root.path(),
        "archive.attestation.json",
        inputs.attestation.as_bytes(),
    )?;
    let record_attestation_path = write_bytes(
        fixture.root.path(),
        "record.attestation.json",
        inputs.attestation.as_bytes(),
    )?;
    let mut output = Command::new("python3");
    output
        .arg(&wrapper)
        .arg(fixture.root.path())
        .arg(&script)
        .arg(&record_path)
        .arg(&archive_path)
        .arg(&result_path)
        .arg(&ci_runs_path)
        .arg(&source_path)
        .arg(&artifact_path)
        .arg(&archive_attestation_path)
        .arg(&record_attestation_path)
        .env("VELNOR_IDENTITY_SOURCE", SOURCE_SHA)
        .env("VELNOR_IDENTITY_REPOSITORY", "tailrocks/velnor-new")
        .env("VELNOR_IDENTITY_REPOSITORY_ID", "1390620900")
        .env("VELNOR_IDENTITY_EVENT", "workflow_dispatch")
        .env("VELNOR_IDENTITY_REF", "refs/heads/main")
        .env("VELNOR_WORKFLOW_REF", WORKFLOW_REF)
        .env("VELNOR_RUN_ID", "7")
        .env("VELNOR_RUN_ATTEMPT", "2")
        .env("VELNOR_ARTIFACT_ID", "9")
        .env("VELNOR_REGISTRY_ACTOR", "actor")
        .env("VELNOR_REGISTRY_TOKEN", "password-value");
    for (name, value) in extra_env {
        output.env(name, value);
    }
    let output = output.output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("evidence program left its exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined, result_path))
}

pub(crate) fn check_evidence_file(fixture: &Fixture, path: &str) -> Result<String, Box<dyn Error>> {
    let output = Command::new("python3")
        .arg(&fixture.driver)
        .arg(fixture.root.path())
        .arg("check_evidence")
        .arg(path)
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("evidence check left no exit code"))?;
    if code != 0 {
        return Err(fail("evidence check rejected its valid input"));
    }
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    require(&combined, "EVIDENCE-VALID", "evidence check")?;
    Ok(combined)
}

#[test]
fn gc1_t51_evidence_caps_control_responses() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let inputs = evidence_inputs();
    let (code, output, _) = run_evidence_program(
        &fixture,
        &inputs,
        &inputs.record,
        &[("VELNOR_STUB_OVERSIZE_TOKEN", "1")],
    )?;
    if code == 0 {
        return Err(fail(
            "evidence program accepted its oversized token response",
        ));
    }
    require(
        &output,
        "Registry response exceeds its size limit.",
        "control cap",
    )?;
    Ok(())
}

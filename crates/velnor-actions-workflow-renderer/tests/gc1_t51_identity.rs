// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Residual 4383697541: wire the materialized identity validators into the
//! VERIFY and EVIDENCE programs instead of leaving them dead code.

use super::*;

const VERIFY_OPEN: &str = "const VERIFY_PROGRAM: &str = r#\"";
const VERIFY_CLOSE: &str = "\n\"#;";
const EVIDENCE_OPEN: &str = "const EVIDENCE_PROGRAM: &str = r#\"";
const EVIDENCE_CLOSE: &str = "\n\"#;";

fn extract_program(open: &str, close: &str, label: &str) -> Result<String, Box<dyn Error>> {
    let start = AUTHORITY_MODULE
        .find(open)
        .ok_or_else(|| fail(&format!("authority lost its {label}")))?
        + open.len();
    let rest = &AUTHORITY_MODULE[start..];
    let end = rest
        .find(close)
        .ok_or_else(|| fail(&format!("authority left its {label} open")))?;
    Ok(rest[..end].to_owned())
}

fn check_verify_wires_identity() -> Result<(), Box<dyn Error>> {
    let program = extract_program(VERIFY_OPEN, VERIFY_CLOSE, "verify program")?;
    for (needle, label) in [
        (
            "import job_image_authority_identity as identity_module",
            "identity import",
        ),
        (
            "identity_module.validate_same_run_candidate(",
            "same-run candidate binding",
        ),
        ("closure_path = Path(sys.argv[4])", "closure input"),
        (
            "Candidate source closure has an incorrect schema.",
            "closure schema binding",
        ),
        (
            "Candidate source closure is not the current run source.",
            "closure source binding",
        ),
    ] {
        require(&program, needle, label)?;
    }
    Ok(())
}

fn check_evidence_wires_identity() -> Result<(), Box<dyn Error>> {
    let program = extract_program(EVIDENCE_OPEN, EVIDENCE_CLOSE, "evidence program")?;
    for (needle, label) in [
        (
            "import job_image_authority_identity as identity_module",
            "identity import",
        ),
        ("identity_module.validate_identity(", "identity validation"),
        (
            "identity_module.validate_candidate_identity(",
            "candidate identity binding",
        ),
        (
            "identity_module.validate_source_inputs(",
            "source input binding",
        ),
        (
            "identity_module.validate_artifact_record(",
            "artifact record binding",
        ),
        ("identity_module.select_ci_run(", "canonical CI selection"),
        (
            "identity_module.validate_attestation_output(",
            "attestation verification",
        ),
        ("VELNOR_IDENTITY_REPOSITORY", "repository identity"),
        ("VELNOR_IDENTITY_REPOSITORY_ID", "repository id identity"),
        ("VELNOR_IDENTITY_EVENT", "event identity"),
        ("VELNOR_IDENTITY_REF", "ref identity"),
        ("ci_runs_path = Path(sys.argv[5])", "CI runs input"),
        (
            "authenticated_source_path = Path(sys.argv[6])",
            "authenticated source input",
        ),
        (
            "artifact_response_path = Path(sys.argv[7])",
            "artifact response input",
        ),
        (
            "archive_attestation_path = Path(sys.argv[8])",
            "archive attestation input",
        ),
        (
            "record_attestation_path = Path(sys.argv[9])",
            "record attestation input",
        ),
    ] {
        require(&program, needle, label)?;
    }
    Ok(())
}

fn run_verify_program(
    fixture: &Fixture,
    record_text: &str,
    archive: &[u8],
    closure_text: Option<&str>,
) -> Result<(i32, String), Box<dyn Error>> {
    let script = fixture.root.path().join("verify_program.py");
    std::fs::write(
        &script,
        extract_program(VERIFY_OPEN, VERIFY_CLOSE, "verify program")?,
    )?;
    let record_path = write_bytes(
        fixture.root.path(),
        "verify-candidate.json",
        record_text.as_bytes(),
    )?;
    let archive_path = write_bytes(fixture.root.path(), "verify.oci.tar", archive)?;
    let mut command = Command::new("python3");
    command
        .arg(&script)
        .arg(fixture.root.path())
        .arg(&record_path)
        .arg(&archive_path);
    if let Some(closure) = closure_text {
        let closure_path = write_bytes(fixture.root.path(), "closure.json", closure.as_bytes())?;
        command.arg(&closure_path);
    }
    let output = command
        .env("VELNOR_EXPECTED_SOURCE", SOURCE_SHA)
        .env("VELNOR_EXPECTED_REF", "refs/heads/main")
        .env("VELNOR_EXPECTED_REPOSITORY", "tailrocks/velnor-new")
        .env("VELNOR_EXPECTED_EVENT", "workflow_dispatch")
        .env("VELNOR_EXPECTED_WORKFLOW_REF", WORKFLOW_REF)
        .env("VELNOR_EXPECTED_RUN_ID", "7")
        .env("VELNOR_EXPECTED_RUN_ATTEMPT", "2")
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("verify program left its exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined))
}

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

fn run_evidence_program(
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

fn check_evidence_file(fixture: &Fixture, path: &str) -> Result<String, Box<dyn Error>> {
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

fn check_rendered_identity_steps(text: &str) -> Result<(), Box<dyn Error>> {
    for (needle, label) in [
        (
            "Fetch canonical CI runs for the candidate source",
            "CI runs step",
        ),
        ("Fetch candidate artifact record", "artifact record step"),
        ("Fetch authenticated source inputs", "source inputs step"),
        (
            "Verify candidate attestations with Sigstore",
            "attestation step",
        ),
        ("VELNOR_IDENTITY_REPOSITORY", "repository identity env"),
        ("VELNOR_IDENTITY_REPOSITORY_ID", "repository id env"),
        ("VELNOR_IDENTITY_EVENT", "event env"),
        ("VELNOR_IDENTITY_REF", "ref env"),
        ("ci.yml/runs?head_sha=", "CI runs query"),
        ("artifacts?per_page=100", "artifact query"),
        (
            "gh attestation verify \\\"candidate/job-image.oci.tar\\\"",
            "archive attestation verification",
        ),
        (
            "gh attestation verify \\\"candidate/job-image-candidate.json\\\"",
            "record attestation verification",
        ),
        ("\\\"$archive\\\" \\\"$closure\\\"", "verify closure wiring"),
    ] {
        require(text, needle, label)?;
    }
    for path in [
        ".velnor/runner.lock",
        "images/job/ubuntu-26.04/Dockerfile",
        "images/job/ubuntu-26.04/README.md",
        "images/job/ubuntu-26.04/build-test.sh",
        "images/job/ubuntu-26.04/candidate/candidate.py",
    ] {
        require(text, path, "authenticated input path")?;
    }
    require_order(
        text,
        "velnor-ci-runs.json",
        "velnor-candidate-artifact.json",
        "evidence input order",
    )?;
    require_order(
        text,
        "velnor-candidate-artifact.json",
        "velnor-authenticated-source.json",
        "evidence input order",
    )?;
    require_order(
        text,
        "velnor-authenticated-source.json",
        "velnor-archive-attestation.json",
        "evidence input order",
    )?;
    require_order(
        text,
        "velnor-archive-attestation.json",
        EVIDENCE_STEP_NAME,
        "evidence input order",
    )?;
    Ok(())
}

#[test]
fn gc1_t51_verify_binds_closure() -> Result<(), Box<dyn Error>> {
    check_verify_wires_identity()?;
    let fixture = fixture()?;
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let record = record_json(&parts, &archive);
    let (code, output) = run_verify_program(&fixture, &record, &archive, Some(&closure_json()))?;
    if code != 0 {
        return Err(fail(&format!(
            "verify program rejected its bound closure:{output}"
        )));
    }
    require(
        &output,
        "fixed authority body accepted the candidate bytes",
        "closure acceptance",
    )?;
    let foreign = closure_json().replacen(SOURCE_SHA, &"b".repeat(40), 1);
    let (code, output) = run_verify_program(&fixture, &record, &archive, Some(&foreign))?;
    if code == 0 {
        return Err(fail("verify program accepted its foreign closure"));
    }
    require(
        &output,
        "Candidate source closure is not the current run source.",
        "closure rejection",
    )?;
    let bad_schema = closure_json().replacen(CLOSURE_SCHEMA, "other.schema.v1", 1);
    let (code, output) = run_verify_program(&fixture, &record, &archive, Some(&bad_schema))?;
    if code == 0 {
        return Err(fail("verify program accepted its misshapen closure"));
    }
    require(
        &output,
        "Candidate source closure has an incorrect schema.",
        "closure schema rejection",
    )?;
    let (code, _) = run_verify_program(&fixture, &record, &archive, None)?;
    if code == 0 {
        return Err(fail("verify program accepted its missing closure"));
    }
    Ok(())
}

#[test]
fn gc1_t51_evidence_wires_identity() -> Result<(), Box<dyn Error>> {
    check_evidence_wires_identity()?;
    check_rendered_identity_steps(&candidate_text()?)?;
    let fixture = fixture()?;
    let inputs = evidence_inputs();
    let (code, output, result) = run_evidence_program(&fixture, &inputs, &inputs.record, &[])?;
    if code != 0 {
        return Err(fail(&format!(
            "evidence program rejected its bound identity:{output}"
        )));
    }
    require(
        &output,
        "fixed authority body assembled the publication evidence",
        "evidence acceptance",
    )?;
    let checked = check_evidence_file(&fixture, &result)?;
    require(
        &checked,
        "EVIDENCE-VALID ghcr.io/tailrocks/velnor-job@sha256:",
        "evidence reference",
    )?;
    Ok(())
}

#[test]
fn gc1_t51_evidence_rejects_unbound_identity() -> Result<(), Box<dyn Error>> {
    let fixture = fixture()?;
    let inputs = evidence_inputs();
    let foreign_record = inputs.record.replacen(SOURCE_SHA, &"b".repeat(40), 1);
    let (code, output, _) = run_evidence_program(&fixture, &inputs, &foreign_record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its foreign source"));
    }
    require(
        &output,
        "Candidate source is not the current main source.",
        "source rejection",
    )?;
    let mut bad_ci = evidence_inputs();
    bad_ci.ci_runs = bad_ci.ci_runs.replacen("\"success\"", "\"failure\"", 1);
    let (code, _, _) = run_evidence_program(&fixture, &bad_ci, &bad_ci.record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its failed CI"));
    }
    let mut bad_inputs = evidence_inputs();
    bad_inputs.authenticated_source =
        bad_inputs
            .authenticated_source
            .replacen(&"0".repeat(64), &"f".repeat(64), 1);
    let (code, output, _) = run_evidence_program(&fixture, &bad_inputs, &bad_inputs.record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its changed inputs"));
    }
    require(
        &output,
        "Build input bytes changed after candidate creation.",
        "inputs rejection",
    )?;
    let mut bad_attestation = evidence_inputs();
    bad_attestation.attestation =
        bad_attestation
            .attestation
            .replacen("1390620900", "1390620901", 1);
    let (code, _, _) =
        run_evidence_program(&fixture, &bad_attestation, &bad_attestation.record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its foreign attestation"));
    }
    let mut bad_artifact = evidence_inputs();
    bad_artifact.artifact_response =
        bad_artifact
            .artifact_response
            .replacen("\"expired\":false", "\"expired\":true", 1);
    let (code, _, _) = run_evidence_program(&fixture, &bad_artifact, &bad_artifact.record, &[])?;
    if code == 0 {
        return Err(fail("evidence program accepted its expired artifact"));
    }
    Ok(())
}

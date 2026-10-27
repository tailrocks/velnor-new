// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::Yaml;

const IDENTITY: &str = include_str!("job_image_authority_identity.py");
const REGISTRY: &str = include_str!("job_image_authority_registry.py");
const PUBLISH: &str = include_str!("job_image_authority_publish.py");
const RECORD: &str = include_str!("job_image_authority_record.py");
const OCI: &str = include_str!("job_image_authority_oci.py");
const TAR: &str = include_str!("job_image_authority_tar.py");

#[derive(Debug)]
struct AuthorityFile {
    path: &'static str,
    source: &'static str,
}

const FILES: [AuthorityFile; 6] = [
    AuthorityFile {
        path: "job_image_authority_identity.py",
        source: IDENTITY,
    },
    AuthorityFile {
        path: "job_image_authority_registry.py",
        source: REGISTRY,
    },
    AuthorityFile {
        path: "job_image_authority_publish.py",
        source: PUBLISH,
    },
    AuthorityFile {
        path: "job_image_authority_record.py",
        source: RECORD,
    },
    AuthorityFile {
        path: "job_image_authority_oci.py",
        source: OCI,
    },
    AuthorityFile {
        path: "job_image_authority_tar.py",
        source: TAR,
    },
];

const DIRECTORY: &str = "$RUNNER_TEMP/velnor-image-authority";
const STEP_NAME: &str = "Materialize fixed image authority implementation";
const END_MARKER: &str = "VELNOR_IMAGE_AUTHORITY_EOF";
pub(super) const VERIFY_STEP_NAME: &str = "Verify candidate bytes with the fixed authority body";
const VERIFY_MARKER: &str = "VELNOR_IMAGE_VERIFY_EOF";
pub(super) const EVIDENCE_STEP_NAME: &str =
    "Assemble publication evidence with the fixed authority body";
const EVIDENCE_MARKER: &str = "VELNOR_IMAGE_EVIDENCE_EOF";

pub(super) fn materialize_step() -> Yaml {
    let mut run = format!("set -eu\ndir=\"{DIRECTORY}\"\nrm -rf \"$dir\"\nmkdir -p \"$dir\"\n");
    for file in FILES {
        run = format!(
            "{run}cat > \"$dir/{}\" <<'{END_MARKER}'\n{}{END_MARKER}\n",
            file.path, file.source
        );
    }
    run.push_str("printf 'VELNOR_IMAGE_AUTHORITY_DIR=%s\\n' \"$dir\" >> \"$GITHUB_ENV\"\n");
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(STEP_NAME)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

const VERIFY_PROGRAM: &str = r#"import hashlib
import json
import os
import sys
sys.path.insert(0, sys.argv[1])
from pathlib import Path
import job_image_authority_identity as identity_module
import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
record_path = Path(sys.argv[2])
archive_path = Path(sys.argv[3])
closure_path = Path(sys.argv[4])
record = record_module.validate_record(record_module.load_record(record_path))
verified = oci_module.verify_oci_archive(archive_path)
hasher = hashlib.sha256()
archive_size = 0
with archive_path.open("rb") as archive_stream:
    while chunk := archive_stream.read(1048576):
        hasher.update(chunk)
        archive_size += len(chunk)
publish_module.check_record_binding(
    record,
    hasher.hexdigest(),
    archive_size,
    verified,
)
try:
    expected_run_id = int(os.environ["VELNOR_EXPECTED_RUN_ID"])
    expected_run_attempt = int(os.environ["VELNOR_EXPECTED_RUN_ATTEMPT"])
except ValueError as error:
    raise publish_module.AuthorityError("Candidate run identity is incorrect.") from error
identity = {
    "repository": os.environ["VELNOR_EXPECTED_REPOSITORY"],
    "event": os.environ["VELNOR_EXPECTED_EVENT"],
    "workflow_ref": os.environ["VELNOR_EXPECTED_WORKFLOW_REF"],
    "ref": os.environ["VELNOR_EXPECTED_REF"],
    "authority_commit": os.environ["VELNOR_EXPECTED_SOURCE"],
    "run_id": expected_run_id,
    "run_attempt": expected_run_attempt,
    "source": os.environ["VELNOR_EXPECTED_SOURCE"],
}
identity_module.validate_same_run_candidate(record, identity)
if closure_path.is_symlink() or not closure_path.is_file():
    raise publish_module.AuthorityError("Candidate source closure must be a regular file.")
if closure_path.stat().st_size > 1048576:
    raise publish_module.AuthorityError("Candidate source closure exceeds its size limit.")
try:
    closure = json.loads(closure_path.read_bytes())
except (UnicodeDecodeError, json.JSONDecodeError) as error:
    raise publish_module.AuthorityError("Candidate source closure has an incorrect format.") from error
if (
    not isinstance(closure, dict)
    or closure.get("schema") != "velnor.job-image-native-source-closure.v1"
):
    raise publish_module.AuthorityError("Candidate source closure has an incorrect schema.")
if closure.get("source_commit") != os.environ["VELNOR_EXPECTED_SOURCE"]:
    raise publish_module.AuthorityError("Candidate source closure is not the current run source.")
print("fixed authority body accepted the candidate bytes")
"#;

pub(super) fn verify_candidate_step(archive: &str, record: &str, closure: &str) -> Yaml {
    let mut run = format!(
        "set -euo pipefail\nauthority=\"$VELNOR_IMAGE_AUTHORITY_DIR\"\narchive=\"{archive}\"\nrecord=\"{record}\"\nclosure=\"{closure}\"\npython3 - \"$authority\" \"$record\" \"$archive\" \"$closure\" <<'{VERIFY_MARKER}'\n"
    );
    run.push_str(VERIFY_PROGRAM);
    run.push_str(VERIFY_MARKER);
    run.push('\n');
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(VERIFY_STEP_NAME)),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "VELNOR_EXPECTED_SOURCE".to_owned(),
                    Yaml::str("${{ github.sha }}"),
                ),
                (
                    "VELNOR_EXPECTED_REF".to_owned(),
                    Yaml::str("${{ github.ref }}"),
                ),
                (
                    "VELNOR_EXPECTED_REPOSITORY".to_owned(),
                    Yaml::str("${{ github.repository }}"),
                ),
                (
                    "VELNOR_EXPECTED_EVENT".to_owned(),
                    Yaml::str("${{ github.event_name }}"),
                ),
                (
                    "VELNOR_EXPECTED_WORKFLOW_REF".to_owned(),
                    Yaml::str(
                        "${{ github.repository }}/.github/workflows/job-image-candidate.yml@${{ github.ref }}",
                    ),
                ),
                (
                    "VELNOR_EXPECTED_RUN_ID".to_owned(),
                    Yaml::str("${{ github.run_id }}"),
                ),
                (
                    "VELNOR_EXPECTED_RUN_ATTEMPT".to_owned(),
                    Yaml::str("${{ github.run_attempt }}"),
                ),
            ]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

const EVIDENCE_PROGRAM: &str = r#"import hashlib
import http.client
import json
import os
import sys
import tempfile
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit
import job_image_authority_identity as identity_module
import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
import job_image_authority_registry as registry_module
RESPONSE_BODY_LIMIT = 65536
class _RegistryResponse:
    def __init__(self, status, headers, body):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body
class _RegistryStream:
    def __init__(self, connection, response):
        self.status = response.status
        self.headers = {key.lower(): value for key, value in response.getheaders()}
        self._connection = connection
        self._response = response
    def read(self, size=-1):
        try:
            return self._response.read(size)
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
    def close(self):
        try:
            self._response.close()
        finally:
            self._connection.close()
def _check_origin(parts):
    allowed = (registry_module.REGISTRY, registry_module.DOWNLOAD_HOST)
    if (
        parts.scheme != "https"
        or parts.hostname not in allowed
        or parts.port not in (None, 443)
        or parts.username is not None
        or parts.password is not None
        or parts.fragment
    ):
        raise publish_module.AuthorityError("Registry URL is outside the fixed HTTPS origin.")
class _RegistryTransport:
    def __init__(self, timeout):
        self.timeout = timeout
    def _connect(self, method, url, headers):
        parts = urlsplit(url)
        _check_origin(parts)
        for name, value in headers.items():
            if "\r" in name + value or "\n" in name + value:
                raise publish_module.AuthorityError("Registry request header has an incorrect format.")
        connection = http.client.HTTPSConnection(parts.hostname, timeout=self.timeout)
        try:
            target = urlunsplit(("", "", parts.path, parts.query, ""))
            connection.putrequest(method, target)
            for name, value in headers.items():
                connection.putheader(name, value)
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            connection.close()
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
        return connection
    def request(self, method, url, headers, body=None, length=0):
        connection = self._connect(method, url, headers)
        try:
            if body is not None:
                if isinstance(body, Path):
                    try:
                        actual = body.stat().st_size
                    except OSError as error:
                        raise publish_module.AuthorityError("Registry request body is missing.") from error
                    if actual != length:
                        raise publish_module.AuthorityError("Registry request body length changed.")
                    connection.putheader("Content-Length", str(length))
                    connection.endheaders()
                    with body.open("rb") as stream:
                        while chunk := stream.read(65536):
                            connection.send(chunk)
                else:
                    if not isinstance(body, bytes) or len(body) != length:
                        raise publish_module.AuthorityError("Registry request body length changed.")
                    connection.putheader("Content-Length", str(length))
                    connection.endheaders()
                    connection.send(body)
            else:
                connection.endheaders()
            response = connection.getresponse()
            chunks = bytearray()
            while len(chunks) <= RESPONSE_BODY_LIMIT:
                chunk = response.read(min(65536, RESPONSE_BODY_LIMIT + 1 - len(chunks)))
                if not chunk:
                    break
                chunks.extend(chunk)
            if len(chunks) > RESPONSE_BODY_LIMIT:
                raise publish_module.AuthorityError("Registry response exceeds its size limit.")
            return _RegistryResponse(response.status, dict(response.getheaders()), bytes(chunks))
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
        finally:
            connection.close()
    def stream(self, method, url, headers):
        connection = self._connect(method, url, headers)
        try:
            connection.endheaders()
            response = connection.getresponse()
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            connection.close()
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
        return _RegistryStream(connection, response)
def _read_limited_json(path, limit, label):
    if path.is_symlink() or not path.is_file():
        raise publish_module.AuthorityError(f"{label} must be a regular file.")
    if path.stat().st_size > limit:
        raise publish_module.AuthorityError(f"{label} exceeds its size limit.")
    with path.open("rb") as stream:
        raw = stream.read(limit + 1)
    if len(raw) > limit:
        raise publish_module.AuthorityError(f"{label} exceeds its size limit.")
    try:
        return json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise publish_module.AuthorityError(f"{label} has an incorrect format.") from error
record_path = Path(sys.argv[2])
archive_path = Path(sys.argv[3])
result_path = Path(sys.argv[4])
ci_runs_path = Path(sys.argv[5])
authenticated_source_path = Path(sys.argv[6])
artifact_response_path = Path(sys.argv[7])
archive_attestation_path = Path(sys.argv[8])
record_attestation_path = Path(sys.argv[9])
try:
    expected_run_id = int(os.environ["VELNOR_RUN_ID"])
    expected_run_attempt = int(os.environ["VELNOR_RUN_ATTEMPT"])
    expected_artifact_id = int(os.environ["VELNOR_ARTIFACT_ID"])
    expected_repository_id = int(os.environ["VELNOR_IDENTITY_REPOSITORY_ID"])
except ValueError as error:
    raise publish_module.AuthorityError("Publication identity is incorrect.") from error
identity = {
    "repository": os.environ["VELNOR_IDENTITY_REPOSITORY"],
    "repository_id": expected_repository_id,
    "event": os.environ["VELNOR_IDENTITY_EVENT"],
    "workflow_ref": os.environ["VELNOR_WORKFLOW_REF"],
    "ref": os.environ["VELNOR_IDENTITY_REF"],
    "authority_commit": os.environ["VELNOR_IDENTITY_SOURCE"],
    "run_id": expected_run_id,
    "run_attempt": expected_run_attempt,
    "source": os.environ["VELNOR_IDENTITY_SOURCE"],
    "artifact_id": expected_artifact_id,
}
identity = identity_module.validate_identity(identity)
record = record_module.validate_record(record_module.load_record(record_path))
identity_module.validate_candidate_identity(record, identity)
authenticated_source = _read_limited_json(authenticated_source_path, 1048576, "Authenticated source")
identity_module.validate_source_inputs(record["source"], authenticated_source)
artifact_response = _read_limited_json(artifact_response_path, 2097152, "Candidate artifact response")
identity_module.validate_artifact_record(artifact_response, identity)
ci_runs = _read_limited_json(ci_runs_path, 2097152, "Canonical CI run response")
ci = identity_module.select_ci_run(ci_runs, identity["source"])
for label, attestation_path in (
    ("Candidate archive", archive_attestation_path),
    ("Candidate record", record_attestation_path),
):
    attestation = _read_limited_json(attestation_path, 1048576, f"{label} attestation")
    identity_module.validate_attestation_output(attestation, identity["source"])
verified = oci_module.verify_oci_archive(archive_path)
hasher = hashlib.sha256()
archive_size = 0
with archive_path.open("rb") as archive_stream:
    while chunk := archive_stream.read(1048576):
        hasher.update(chunk)
        archive_size += len(chunk)
archive_digest = hasher.hexdigest()
publish_module.check_record_binding(
    record,
    archive_digest,
    archive_size,
    verified,
)
manifest = publish_module.read_manifest_bytes(
    archive_path,
    verified["platform_manifest_digest"],
    verified["platform_manifest_size_bytes"],
)
manifest_value = json.loads(manifest)
blob_paths = {}
with tempfile.TemporaryDirectory(prefix="velnor-image-publish-") as temp:
    for digest, size in publish_module.blob_descriptors(manifest_value).items():
        blob_paths[digest] = publish_module.extract_blob(archive_path, digest, size, Path(temp))
    transport = _RegistryTransport(30)
    token = publish_module.fetch_registry_token(
        registry_module,
        transport,
        os.environ["VELNOR_REGISTRY_ACTOR"],
        os.environ["VELNOR_REGISTRY_TOKEN"],
    )
    proof = publish_module.publish_image(registry_module, transport, token, manifest, blob_paths)
record_raw = record_path.read_bytes()
evidence = publish_module.build_evidence(
    identity,
    ci,
    {
        "sha256": archive_digest,
        "size_bytes": archive_size,
    },
    verified,
    manifest,
    hashlib.sha256(record_raw).hexdigest(),
    record["source"],
    proof,
)
result_path.write_bytes(evidence)
print("fixed authority body assembled the publication evidence")
"#;

pub(super) const CI_RUNS_PATH: &str = "$RUNNER_TEMP/velnor-ci-runs.json";
pub(super) const AUTHENTICATED_SOURCE_PATH: &str = "$RUNNER_TEMP/velnor-authenticated-source.json";
pub(super) const ARTIFACT_RESPONSE_PATH: &str = "$RUNNER_TEMP/velnor-candidate-artifact.json";
pub(super) const ARCHIVE_ATTESTATION_PATH: &str = "$RUNNER_TEMP/velnor-archive-attestation.json";
pub(super) const RECORD_ATTESTATION_PATH: &str = "$RUNNER_TEMP/velnor-record-attestation.json";

pub(super) fn assemble_evidence_step(
    archive: &str,
    record: &str,
    result: &str,
    artifact_id: &str,
) -> Yaml {
    let mut run = format!(
        "set -euo pipefail\nauthority=\"$VELNOR_IMAGE_AUTHORITY_DIR\"\narchive=\"{archive}\"\nrecord=\"{record}\"\nresult=\"{result}\"\nci_runs=\"{CI_RUNS_PATH}\"\nauthenticated_source=\"{AUTHENTICATED_SOURCE_PATH}\"\nartifact_response=\"{ARTIFACT_RESPONSE_PATH}\"\narchive_attestation=\"{ARCHIVE_ATTESTATION_PATH}\"\nrecord_attestation=\"{RECORD_ATTESTATION_PATH}\"\npython3 - \"$authority\" \"$record\" \"$archive\" \"$result\" \"$ci_runs\" \"$authenticated_source\" \"$artifact_response\" \"$archive_attestation\" \"$record_attestation\" <<'{EVIDENCE_MARKER}'\n"
    );
    run.push_str(EVIDENCE_PROGRAM);
    run.push_str(EVIDENCE_MARKER);
    run.push('\n');
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(EVIDENCE_STEP_NAME)),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "VELNOR_IDENTITY_SOURCE".to_owned(),
                    Yaml::str("${{ github.sha }}"),
                ),
                (
                    "VELNOR_IDENTITY_REPOSITORY".to_owned(),
                    Yaml::str("${{ github.repository }}"),
                ),
                (
                    "VELNOR_IDENTITY_REPOSITORY_ID".to_owned(),
                    Yaml::str("${{ env.GITHUB_REPOSITORY_ID }}"),
                ),
                (
                    "VELNOR_IDENTITY_EVENT".to_owned(),
                    Yaml::str("${{ github.event_name }}"),
                ),
                (
                    "VELNOR_IDENTITY_REF".to_owned(),
                    Yaml::str("${{ github.ref }}"),
                ),
                (
                    "VELNOR_WORKFLOW_REF".to_owned(),
                    Yaml::str(
                        "${{ github.repository }}/.github/workflows/job-image-candidate.yml@${{ github.ref }}",
                    ),
                ),
                (
                    "VELNOR_RUN_ID".to_owned(),
                    Yaml::str("${{ github.run_id }}"),
                ),
                (
                    "VELNOR_RUN_ATTEMPT".to_owned(),
                    Yaml::str("${{ github.run_attempt }}"),
                ),
                ("VELNOR_ARTIFACT_ID".to_owned(), Yaml::str(artifact_id)),
                (
                    "VELNOR_REGISTRY_ACTOR".to_owned(),
                    Yaml::str("${{ github.actor }}"),
                ),
                (
                    "VELNOR_REGISTRY_TOKEN".to_owned(),
                    Yaml::str("${{ secrets.GITHUB_TOKEN }}"),
                ),
            ]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

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
import sys
sys.path.insert(0, sys.argv[1])
from pathlib import Path
import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
record_path = Path(sys.argv[2])
archive_path = Path(sys.argv[3])
record = record_module.validate_record(record_module.load_record(record_path))
verified = oci_module.verify_oci_archive(archive_path)
raw = archive_path.read_bytes()
publish_module.check_record_binding(
    record,
    hashlib.sha256(raw).hexdigest(),
    len(raw),
    verified,
)
print("fixed authority body accepted the candidate bytes")
"#;

pub(super) fn verify_candidate_step(archive: &str, record: &str) -> Yaml {
    let mut run = format!(
        "set -euo pipefail\nauthority=\"$VELNOR_IMAGE_AUTHORITY_DIR\"\narchive=\"{archive}\"\nrecord=\"{record}\"\npython3 - \"$authority\" \"$record\" \"$archive\" <<'{VERIFY_MARKER}'\n"
    );
    run.push_str(VERIFY_PROGRAM);
    run.push_str(VERIFY_MARKER);
    run.push('\n');
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(VERIFY_STEP_NAME)),
        ("shell".to_owned(), Yaml::str("bash")),
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
import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
import job_image_authority_registry as registry_module
class _RegistryResponse:
    def __init__(self, status, headers, body):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body
class _RegistryTransport:
    def __init__(self, timeout):
        self.timeout = timeout
    def request(self, method, url, headers, body=None, length=0):
        parts = urlsplit(url)
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
        for name, value in headers.items():
            if "\r" in name + value or "\n" in name + value:
                raise publish_module.AuthorityError("Registry request header has an incorrect format.")
        connection = http.client.HTTPSConnection(parts.hostname, timeout=self.timeout)
        try:
            target = urlunsplit(("", "", parts.path, parts.query, ""))
            connection.putrequest(method, target)
            for name, value in headers.items():
                connection.putheader(name, value)
            if body is not None:
                if not isinstance(body, bytes) or len(body) != length:
                    raise publish_module.AuthorityError("Registry request body length changed.")
                connection.putheader("Content-Length", str(length))
            connection.endheaders()
            if body is not None:
                connection.send(body)
            response = connection.getresponse()
            chunks = bytearray()
            limit = publish_module.MAX_UPLOAD_BYTES + 1
            while True:
                chunk = response.read(min(65536, limit - len(chunks)))
                if not chunk:
                    break
                chunks.extend(chunk)
                if len(chunks) > publish_module.MAX_UPLOAD_BYTES:
                    raise publish_module.AuthorityError("Registry response exceeds its size limit.")
            return _RegistryResponse(response.status, dict(response.getheaders()), bytes(chunks))
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            raise publish_module.AuthorityError("Registry request failed or timed out.") from error
        finally:
            connection.close()
record_path = Path(sys.argv[2])
archive_path = Path(sys.argv[3])
result_path = Path(sys.argv[4])
record = record_module.validate_record(record_module.load_record(record_path))
verified = oci_module.verify_oci_archive(archive_path)
raw = archive_path.read_bytes()
publish_module.check_record_binding(
    record,
    hashlib.sha256(raw).hexdigest(),
    len(raw),
    verified,
)
manifest = publish_module.read_manifest_bytes(
    archive_path,
    verified["platform_manifest_digest"],
    verified["platform_manifest_size_bytes"],
)
manifest_value = json.loads(manifest)
blobs = {}
with tempfile.TemporaryDirectory(prefix="velnor-image-publish-") as temp:
    for digest, size in publish_module.blob_descriptors(manifest_value).items():
        blobs[digest] = publish_module.extract_blob(archive_path, digest, size, Path(temp)).read_bytes()
    transport = _RegistryTransport(30)
    token = publish_module.fetch_registry_token(
        registry_module,
        transport,
        os.environ["VELNOR_REGISTRY_ACTOR"],
        os.environ["VELNOR_REGISTRY_TOKEN"],
    )
    proof = publish_module.publish_image(registry_module, transport, token, manifest, blobs)
record_raw = record_path.read_bytes()
identity = {
    "source": os.environ["VELNOR_IDENTITY_SOURCE"],
    "workflow_ref": os.environ["VELNOR_WORKFLOW_REF"],
    "run_id": int(os.environ["VELNOR_RUN_ID"]),
    "run_attempt": int(os.environ["VELNOR_RUN_ATTEMPT"]),
    "artifact_id": int(os.environ["VELNOR_ARTIFACT_ID"]),
}
ci = {
    "run_id": int(os.environ["VELNOR_RUN_ID"]),
    "run_attempt": int(os.environ["VELNOR_RUN_ATTEMPT"]),
}
record_archive = {
    "sha256": hashlib.sha256(raw).hexdigest(),
    "size_bytes": len(raw),
}
evidence = publish_module.build_evidence(
    identity,
    ci,
    record_archive,
    verified,
    manifest,
    hashlib.sha256(record_raw).hexdigest(),
    record["source"],
    proof,
)
result_path.write_bytes(evidence)
print("fixed authority body assembled the publication evidence")
"#;

pub(super) fn assemble_evidence_step(
    archive: &str,
    record: &str,
    result: &str,
    artifact_id: &str,
) -> Yaml {
    let mut run = format!(
        "set -euo pipefail\nauthority=\"$VELNOR_IMAGE_AUTHORITY_DIR\"\narchive=\"{archive}\"\nrecord=\"{record}\"\nresult=\"{result}\"\npython3 - \"$authority\" \"$record\" \"$archive\" \"$result\" <<'{EVIDENCE_MARKER}'\n"
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

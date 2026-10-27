// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

use velnor_actions_actionlint::actions::MISE_ACTION_SHA;
use velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script;
use velnor_actions_contract::{RoutingWorkflow, Step, StepKind};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::steps::{
    ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES,
};
use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;
use velnor_actions_workflow_renderer::{
    ProductReleasePins, RenderedFile, Schema2WorkflowRequest, VERIFY_PATH, render_schema2_workflows,
};

const IDENTITY_SOURCE: &str = include_str!("../src/job_image_authority_identity.py");
const REGISTRY_SOURCE: &str = include_str!("../src/job_image_authority_registry.py");
const PUBLISH_SOURCE: &str = include_str!("../src/job_image_authority_publish.py");
const RECORD_SOURCE: &str = include_str!("../src/job_image_authority_record.py");
const OCI_SOURCE: &str = include_str!("../src/job_image_authority_oci.py");
const TAR_SOURCE: &str = include_str!("../src/job_image_authority_tar.py");

const IDENTITY_FILE: &str = "job_image_authority_identity.py";
const REGISTRY_FILE: &str = "job_image_authority_registry.py";
const PUBLISH_FILE: &str = "job_image_authority_publish.py";
const RECORD_FILE: &str = "job_image_authority_record.py";
const OCI_FILE: &str = "job_image_authority_oci.py";
const TAR_FILE: &str = "job_image_authority_tar.py";

const STEP_NAME: &str = "Materialize fixed image authority implementation";
const VERIFY_STEP_NAME: &str = "Verify candidate bytes with the fixed authority body";
const EVIDENCE_STEP_NAME: &str = "Assemble publication evidence with the fixed authority body";
const BODY_DIRECTORY: &str = "$RUNNER_TEMP/velnor-image-authority";
const END_MARKER: &str = "VELNOR_IMAGE_AUTHORITY_EOF";
const VERIFY_MARKER: &str = "VELNOR_IMAGE_VERIFY_EOF";
const EVIDENCE_MARKER: &str = "VELNOR_IMAGE_EVIDENCE_EOF";
const CANDIDATE_PATH: &str = ".github/workflows/job-image-candidate.yml";

const DRIVER: &str = r#"import hashlib
import json
import sys
import tempfile
from pathlib import Path
from urllib.parse import parse_qsl, urlsplit

ROOT = Path(sys.argv[1])

sys.path.insert(0, str(ROOT))

import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
import job_image_authority_registry as registry_module


class _FakeResponse:
    def __init__(self, status, headers, body):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self.body = body


class _FakeStream:
    def __init__(self, status, headers, data):
        self.status = status
        self.headers = {key.lower(): value for key, value in headers.items()}
        self._data = data
        self._offset = 0

    def read(self, size=-1):
        if size is None or size < 0:
            size = len(self._data) - self._offset
        chunk = self._data[self._offset:self._offset + size]
        self._offset += len(chunk)
        return chunk

    def close(self):
        pass


class _FakeRegistry:
    """In-memory OCI registry for the data-only flow test; no network is opened."""

    def __init__(self):
        self.store = {}
        self.puts = 0

    def _key(self, path):
        digest = path.rsplit("/", 1)[-1]
        kind = "manifests" if "/manifests/" in path else "blobs"
        return f"{kind}:{digest}", digest

    def request(self, method, url, headers, body=None, length=0):
        parts = urlsplit(url)
        path = parts.path
        if method == "GET" and path == "/v2/":
            return _FakeResponse(401, {"www-authenticate": 'Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:tailrocks/velnor-job:pull,push"'}, b"")
        if method == "GET" and path == "/token":
            return _FakeResponse(200, {}, b'{"token": "flow-token-0123456789abcdef"}')
        if method == "HEAD":
            key, digest = self._key(path)
            if key in self.store:
                data = self.store[key]
                return _FakeResponse(
                    200,
                    {
                        "docker-content-digest": digest,
                        "content-length": str(len(data)),
                    },
                    b"",
                )
            return _FakeResponse(404, {}, b"")
        if method == "POST":
            return _FakeResponse(
                202,
                {"location": "/v2/tailrocks/velnor-job/blobs/uploads/flow-uuid"},
                b"",
            )
        if method == "PUT":
            query = dict(parse_qsl(parts.query))
            if "/manifests/" in path:
                key, digest = self._key(path)
                if not isinstance(body, bytes) or len(body) != length:
                    raise AssertionError("manifest PUT must send exact bytes")
                self.store[key] = body
                self.puts += 1
                return _FakeResponse(201, {"docker-content-digest": digest}, b"")
            if isinstance(body, (bytes, bytearray, memoryview)):
                raise AssertionError("blob PUT must stream a temp file, not bytes")
            chunks = []
            with open(body, "rb") as stream:
                while chunk := stream.read(65536):
                    chunks.append(chunk)
            data = b"".join(chunks)
            if len(data) != length:
                raise AssertionError("streamed blob PUT length changed")
            self.store[f"blobs:{query['digest']}"] = data
            self.puts += 1
            return _FakeResponse(201, {}, b"")
        if method == "GET":
            raise AssertionError("blob and manifest GET must use the streaming call")
        raise AssertionError(f"unexpected registry call: {method} {url}")

    def stream(self, method, url, headers):
        if method != "GET":
            raise AssertionError(f"unexpected stream call: {method} {url}")
        key, digest = self._key(urlsplit(url).path)
        stored = self.store[key]
        return _FakeStream(
            200,
            {"docker-content-digest": digest, "content-length": str(len(stored))},
            stored,
        )


def main():
    record_path = Path(sys.argv[2])
    archive_path = Path(sys.argv[3])
    identity_path = Path(sys.argv[4])
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
    blob_paths = {}
    with tempfile.TemporaryDirectory(prefix="gc1-t48-flow-") as temp:
        for digest, size in publish_module.blob_descriptors(manifest_value).items():
            blob_paths[digest] = publish_module.extract_blob(
                archive_path, digest, size, Path(temp)
            )
        proof = publish_module.publish_image(
            registry_module,
            _FakeRegistry(),
            "flow-token-0123456789abcdef",
            manifest,
            blob_paths,
        )
    identity = json.loads(identity_path.read_text())
    ci = {"run_id": identity["run_id"], "run_attempt": identity["run_attempt"]}
    record_raw = record_path.read_bytes()
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
    (ROOT / "evidence.json").write_bytes(evidence)
    publish_module.validate_evidence(json.loads(evidence))
    print("FLOW-OK")
    print("puts=" + str(proof["manifest"]["size"]))
    print("digest_reference=" + json.loads(evidence)["digest_reference"])


main()
"#;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

fn require_count(
    haystack: &str,
    needle: &str,
    expected: usize,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let actual = haystack.matches(needle).count();
    if actual == expected {
        Ok(())
    } else {
        Err(fail(&format!(
            "{label} left:{actual}:expected:{expected}:{needle}"
        )))
    }
}

fn require_order(
    haystack: &str,
    first: &str,
    second: &str,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let first_pos = haystack
        .find(first)
        .ok_or_else(|| fail(&format!("{label} lost:{first}")))?;
    let second_pos = haystack
        .find(second)
        .ok_or_else(|| fail(&format!("{label} lost:{second}")))?;
    if first_pos < second_pos {
        Ok(())
    } else {
        Err(fail(&format!("{label} misordered:{first}:{second}")))
    }
}

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

fn fixture_step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn verify_rust_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &format!("{} exit 0", credential_unset_prelude()),
    ])
}

fn verify_lint_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &jactionlint_verifier_script("${VELNOR_TOOL_JACTIONLINT}", VERIFY_PATH),
    ])
}

fn install_runner_build_tools_argv() -> Vec<String> {
    argv(&["mise", "install", "rust@1.91.0"])
}

fn install_resource_probe_target_argv() -> Vec<String> {
    argv(&["rustup", "target", "add", "x86_64-unknown-linux-musl"])
}

fn runner_build_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--release",
        "--package",
        "velnor-runner-cli",
        "--bin",
        "velnor-host",
    ])
}

fn resource_probe_build_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--package",
        "velnor-resource-probe",
        "--bin",
        "velnor-resource-probe",
        "--release",
        "--target",
        "x86_64-unknown-linux-musl",
    ])
}

fn install_resource_probe_release_tools_argv() -> Vec<String> {
    argv(&["mise", "install", "rust@1.91.0", "mr-boxington@1.0.0"])
}

fn resource_probe_release_build_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--package",
        "velnor-runner-host",
        "--bin",
        "velnor-resource-probe-release",
        "--target",
        "x86_64-unknown-linux-gnu",
    ])
}

fn resource_probe_release_test_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "nextest",
        "run",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--package",
        "velnor-runner-host",
        "--test",
        "resource_probe_release_public",
        "--target",
        "x86_64-unknown-linux-gnu",
    ])
}

#[path = "gc1_t48_archive.rs"]
mod gc1_t48_archive;
#[path = "gc1_t48_checks.rs"]
mod gc1_t48_checks;
#[path = "gc1_t48_flow.rs"]
mod gc1_t48_flow;
#[path = "gc1_t48_pins.rs"]
mod gc1_t48_pins;
pub(crate) use gc1_t48_archive::*;
pub(crate) use gc1_t48_checks::*;
pub(crate) use gc1_t48_pins::*;

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
const AUTHORITY_MODULE: &str = include_str!("../src/job_image_authority.rs");

const IDENTITY_FILE: &str = "job_image_authority_identity.py";
const REGISTRY_FILE: &str = "job_image_authority_registry.py";
const PUBLISH_FILE: &str = "job_image_authority_publish.py";
const RECORD_FILE: &str = "job_image_authority_record.py";
const OCI_FILE: &str = "job_image_authority_oci.py";
const TAR_FILE: &str = "job_image_authority_tar.py";

const STEP_NAME: &str = "Materialize fixed image authority implementation";
const BODY_DIRECTORY: &str = "$RUNNER_TEMP/velnor-image-authority";
const END_MARKER: &str = "VELNOR_IMAGE_AUTHORITY_EOF";
const CANDIDATE_PATH: &str = ".github/workflows/job-image-candidate.yml";

const DRIVER: &str = r#"import json
import sys
import tempfile
from pathlib import Path

ROOT = Path(sys.argv[1])

sys.path.insert(0, str(ROOT))

import job_image_authority_identity as identity_module
import job_image_authority_oci as oci_module
import job_image_authority_publish as publish_module
import job_image_authority_record as record_module
import job_image_authority_registry as registry_module
import job_image_authority_tar as tar_module

ERRORS = (
    identity_module.AuthorityError,
    oci_module.AuthorityError,
    publish_module.AuthorityError,
    record_module.AuthorityError,
    registry_module.AuthorityError,
    tar_module.AuthorityError,
)


def envelope():
    return json.loads(Path(sys.argv[3]).read_text())


def report(marker, detail):
    print(marker)
    if detail:
        print(detail)


def run_identity(case):
    try:
        identity_module.validate_identity(case)
    except ERRORS as error:
        report("IDENTITY-REJECTED", error)
        return
    report("IDENTITY-OK", case["source"])


def run_source_inputs(case):
    try:
        commit = identity_module.validate_source_inputs(case["record_source"], case["authenticated"])
    except ERRORS as error:
        report("SOURCE-INPUTS-REJECTED", error)
        return
    report("SOURCE-INPUTS-OK", commit)


def run_build_lock(case):
    try:
        identity_module.validate_build_lock(case["builder"], case["buildx_version"])
    except ERRORS as error:
        report("BUILD-LOCK-REJECTED", error)
        return
    report("BUILD-LOCK-OK", case["buildx_version"])


def run_redirect(case):
    try:
        target = registry_module.redirect_target(case["path"], case["digest"], case["status"], case["location"])
    except ERRORS as error:
        report("REDIRECT-REJECTED", error)
        return
    if target is None:
        report("REDIRECT-NONE", "direct")
    else:
        report("REDIRECT-OK", target)


def run_record_ok(path):
    record_module.validate_record(record_module.load_record(Path(path)))
    print("RECORD-OK")


def run_record_bad(path):
    try:
        record_module.validate_record(record_module.load_record(Path(path)))
    except ERRORS as error:
        print(f"RECORD-REJECTED {error}")
        return
    raise SystemExit(3)


def run_archive_ok(path):
    result = oci_module.verify_oci_archive(Path(path))
    print("ARCHIVE-OK")
    print(f"archive_index_digest={result['archive_index_digest']}")
    print(f"platform_manifest_digest={result['platform_manifest_digest']}")
    print(f"layer_digests={','.join(result['layer_digests'])}")


def run_archive_bad(path):
    try:
        oci_module.verify_oci_archive(Path(path))
    except ERRORS as error:
        print(f"ARCHIVE-REJECTED {error}")
        return
    raise SystemExit(3)


def run_tar_member(name):
    try:
        first = tar_module._safe_member(name)
    except ERRORS as error:
        try:
            oci_module._safe_member(name)
        except ERRORS as other:
            if str(other) != str(error):
                raise SystemExit(4)
            print(f"TAR-MEMBER-REJECTED {error}")
            return
        raise SystemExit(4)
    try:
        second = oci_module._safe_member(name)
    except ERRORS:
        raise SystemExit(4)
    if second != first:
        raise SystemExit(4)
    print(f"TAR-MEMBER-OK {first}")


def run_descriptors(case):
    try:
        unique = publish_module.blob_descriptors(case["manifest"])
    except ERRORS as error:
        report("DESCRIPTORS-REJECTED", error)
        return
    report("DESCRIPTORS-OK", f"digests={','.join(sorted(unique))}")


def run_manifest(case):
    try:
        raw = publish_module.read_manifest_bytes(Path(case["archive"]), case["digest"], case["size"])
    except ERRORS as error:
        report("MANIFEST-REJECTED", error)
        return
    report("MANIFEST-OK", f"size={len(raw)}")


def run_extract(case):
    try:
        with tempfile.TemporaryDirectory(prefix="gc1-t47-extract-") as temp:
            output = publish_module.extract_blob(
                Path(case["archive"]), case["digest"], case["size"], Path(temp)
            )
            data = Path(output).read_bytes()
    except ERRORS as error:
        report("EXTRACT-REJECTED", error)
        return
    import hashlib

    report("EXTRACT-OK", f"sha={hashlib.sha256(data).hexdigest()} size={len(data)}")


def run_evidence(case):
    try:
        encoded = publish_module.build_evidence(
            case["identity"],
            case["ci"],
            case["record_archive"],
            case["oci"],
            bytes.fromhex(case["manifest_hex"]),
            case["record_sha256"],
        )
    except ERRORS as error:
        report("EVIDENCE-REJECTED", error)
        return
    (ROOT / "evidence.json").write_bytes(encoded)
    report("EVIDENCE-OK", f"size={len(encoded)}")


def run_evidence_check(case):
    try:
        publish_module.validate_evidence(case["evidence"])
    except ERRORS as error:
        report("EVIDENCE-REJECTED", error)
        return
    report("EVIDENCE-VALID", case["evidence"]["digest_reference"])


def main():
    mode = sys.argv[2]
    if mode in (
        "identity", "source_inputs", "build_lock", "redirect", "descriptors",
        "manifest", "extract", "evidence", "evidence_check",
    ):
        {
            "identity": run_identity,
            "source_inputs": run_source_inputs,
            "build_lock": run_build_lock,
            "redirect": run_redirect,
            "descriptors": run_descriptors,
            "manifest": run_manifest,
            "extract": run_extract,
            "evidence": run_evidence,
            "evidence_check": run_evidence_check,
        }[mode](envelope())
    elif mode in ("record_ok", "record_bad", "archive_ok", "archive_bad"):
        {
            "record_ok": run_record_ok,
            "record_bad": run_record_bad,
            "archive_ok": run_archive_ok,
            "archive_bad": run_archive_bad,
        }[mode](sys.argv[3])
    elif mode == "tar_member":
        run_tar_member(sys.argv[3])
    else:
        raise SystemExit(2)


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

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

#[path = "gc1_t47_archive.rs"]
mod gc1_t47_archive;
#[path = "gc1_t47_argv.rs"]
mod gc1_t47_argv;
#[path = "gc1_t47_inputs.rs"]
mod gc1_t47_inputs;
#[path = "gc1_t47_reject.rs"]
mod gc1_t47_reject;
#[path = "gc1_t47_shared.rs"]
mod gc1_t47_shared;
pub(crate) use gc1_t47_archive::*;
pub(crate) use gc1_t47_argv::*;
pub(crate) use gc1_t47_inputs::*;
pub(crate) use gc1_t47_shared::*;

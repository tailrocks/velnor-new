// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! F5 job-image-authority follow-up (review thread 4389237067 + residuals).
//!
//! * Streaming: the publication path must pass blob temp files through the
//!   registry transport instead of retaining every blob in memory.
//! * Identity: the VERIFY/EVIDENCE programs must import and call the
//!   materialized identity validators instead of leaving them dead code.
//! * Record binding: the strongest binding the repo provides for the record
//!   (Sigstore attestation verification) must gate publication evidence.
//! * Trust root: the fixed authority body stays the sole gate before
//!   attestation and publication; no generic final fan-in may interpose.

use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_actionlint::actions::MISE_ACTION_SHA;
use velnor_actions_contract::config::jactionlint_verifier::jactionlint_verifier_script;
use velnor_actions_contract::{RoutingWorkflow, Step, StepKind};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};
use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;
use velnor_actions_workflow_renderer::{
    ProductReleasePins, Schema2WorkflowRequest, VERIFY_PATH, render_schema2_workflows,
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

const CANDIDATE_PATH: &str = ".github/workflows/job-image-candidate.yml";
const VERIFY_STEP_NAME: &str = "Verify candidate bytes with the fixed authority body";
const EVIDENCE_STEP_NAME: &str = "Assemble publication evidence with the fixed authority body";

const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const WORKFLOW_REF: &str =
    "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml@refs/heads/main";
const CLOSURE_SCHEMA: &str = "velnor.job-image-native-source-closure.v1";

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

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

fn tar_entry(name: &str, data: &[u8], typeflag: u8) -> Vec<u8> {
    let mut header = [0u8; 512];
    let name_bytes = name.as_bytes();
    header[..name_bytes.len()].copy_from_slice(name_bytes);
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    let size_field = format!("{:011o}\0", data.len());
    header[124..136].copy_from_slice(size_field.as_bytes());
    header[148..156].copy_from_slice(b"        ");
    header[156] = typeflag;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let checksum_field = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(checksum_field.as_bytes());
    let mut entry = header.to_vec();
    entry.extend_from_slice(data);
    while entry.len() % 512 != 0 {
        entry.push(0);
    }
    entry
}

fn tar_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = Vec::new();
    for (name, data) in entries {
        archive.extend_from_slice(&tar_entry(name, data, b'0'));
    }
    archive.extend_from_slice(&[0u8; 1024]);
    archive
}

pub(crate) struct ImageParts {
    pub(crate) layer: Vec<u8>,
    pub(crate) config: Vec<u8>,
    pub(crate) manifest: Vec<u8>,
    pub(crate) index: Vec<u8>,
    pub(crate) layout: Vec<u8>,
}

pub(crate) fn image_parts() -> ImageParts {
    let layer = tar_archive(&[("hello.txt", b"hello".as_slice())]);
    let config = format!(
        "{{\"architecture\":\"amd64\",\"os\":\"linux\",\"rootfs\":{{\"type\":\"layers\",\"diff_ids\":[\"sha256:{}\"]}}}}",
        sha256_hex(&layer)
    )
    .into_bytes();
    let config_hex = sha256_hex(&config);
    let layer_hex = sha256_hex(&layer);
    let manifest = format!(
        "{{\"schemaVersion\":2,\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\",\"config\":{{\"mediaType\":\"application/vnd.oci.image.config.v1+json\",\"digest\":\"sha256:{config_hex}\",\"size\":{}}},\"layers\":[{{\"mediaType\":\"application/vnd.oci.image.layer.v1.tar\",\"digest\":\"sha256:{layer_hex}\",\"size\":{}}}]}}",
        config.len(),
        layer.len(),
    )
    .into_bytes();
    let index = format!(
        "{{\"schemaVersion\":2,\"mediaType\":\"application/vnd.oci.image.index.v1+json\",\"manifests\":[{{\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\",\"digest\":\"sha256:{}\",\"size\":{},\"platform\":{{\"architecture\":\"amd64\",\"os\":\"linux\"}}}}]}}",
        sha256_hex(&manifest),
        manifest.len(),
    )
    .into_bytes();
    let layout = b"{\"imageLayoutVersion\":\"1.0.0\"}".to_vec();
    ImageParts {
        layer,
        config,
        manifest,
        index,
        layout,
    }
}

pub(crate) fn archive_bytes(parts: &ImageParts) -> Vec<u8> {
    let manifest_hex = sha256_hex(&parts.manifest);
    let config_hex = sha256_hex(&parts.config);
    let layer_hex = sha256_hex(&parts.layer);
    tar_archive(&[
        ("oci-layout", &parts.layout),
        ("index.json", &parts.index),
        (&format!("blobs/sha256/{manifest_hex}"), &parts.manifest),
        (&format!("blobs/sha256/{config_hex}"), &parts.config),
        (&format!("blobs/sha256/{layer_hex}"), &parts.layer),
    ])
}

/// Record fixture bound to the canonical publish identity: `workflow_dispatch`
/// on refs/heads/main with the canonical repository id and the candidate
/// workflow ref, so the identity validators accept it.
pub(crate) fn record_json(parts: &ImageParts, archive: &[u8]) -> String {
    let manifest_hex = sha256_hex(&parts.manifest);
    let config_hex = sha256_hex(&parts.config);
    let layer_hex = sha256_hex(&parts.layer);
    format!(
        "{{\"schema\":\"velnor.job-image-candidate.v1\",\"source\":{{\"commit\":\"{SOURCE_SHA}\",\"ref\":\"refs/heads/main\",\"inputs\":{{\".velnor/runner.lock\":\"{lock0}\",\"images/job/ubuntu-26.04/Dockerfile\":\"{lock1}\",\"images/job/ubuntu-26.04/README.md\":\"{lock2}\",\"images/job/ubuntu-26.04/build-test.sh\":\"{lock3}\",\"images/job/ubuntu-26.04/candidate/candidate.py\":\"{lock4}\"}}}}",
        lock0 = "0".repeat(64),
        lock1 = "1".repeat(64),
        lock2 = "2".repeat(64),
        lock3 = "3".repeat(64),
        lock4 = "4".repeat(64),
    ) + &format!(
        ",\"workflow\":{{\"repository\":\"tailrocks/velnor-new\",\"repository_id\":1390620900,\"event\":\"workflow_dispatch\",\"workflow_ref\":\"{WORKFLOW_REF}\",\"authority_commit\":\"{SOURCE_SHA}\",\"run_id\":7,\"run_attempt\":2}},\"platform\":\"linux/amd64\",\"build\":{{\"builder\":\"github.com/docker/buildx v0.25.0\",\"platform\":\"linux/amd64\",\"arguments\":[],\"exporter\":\"oci\",\"provenance\":\"disabled\",\"compression\":\"gzip\",\"force_compression\":true}},\"archive\":{{\"sha256\":\"{}\",\"size_bytes\":{}}},\"oci\":{{\"archive_index_digest\":\"{}\",\"archive_index_size_bytes\":{},\"platform_manifest_digest\":\"sha256:{}\",\"platform_manifest_size_bytes\":{},\"image_configuration_digest\":\"sha256:{}\",\"image_configuration_size_bytes\":{},\"layer_digests\":[\"sha256:{}\"]}}}}",
        sha256_hex(archive),
        archive.len(),
        sha256_hex(&parts.index),
        parts.index.len(),
        manifest_hex,
        parts.manifest.len(),
        config_hex,
        parts.config.len(),
        layer_hex,
    )
}

pub(crate) fn closure_json() -> String {
    format!(
        "{{\"schema\":\"{CLOSURE_SCHEMA}\",\"source_commit\":\"{SOURCE_SHA}\",\"inputs\":{{}}}}"
    )
}

pub(crate) fn authenticated_source_json() -> String {
    format!(
        "{{\"commit\":\"{SOURCE_SHA}\",\"inputs\":{{\".velnor/runner.lock\":\"{lock0}\",\"images/job/ubuntu-26.04/Dockerfile\":\"{lock1}\",\"images/job/ubuntu-26.04/README.md\":\"{lock2}\",\"images/job/ubuntu-26.04/build-test.sh\":\"{lock3}\",\"images/job/ubuntu-26.04/candidate/candidate.py\":\"{lock4}\"}}}}",
        lock0 = "0".repeat(64),
        lock1 = "1".repeat(64),
        lock2 = "2".repeat(64),
        lock3 = "3".repeat(64),
        lock4 = "4".repeat(64),
    )
}

pub(crate) fn ci_runs_json() -> String {
    format!(
        "{{\"workflow_runs\":[{{\"path\":\".github/workflows/ci.yml\",\"event\":\"push\",\"head_branch\":\"main\",\"head_sha\":\"{SOURCE_SHA}\",\"head_repository\":{{\"full_name\":\"tailrocks/velnor-new\"}},\"run_number\":3,\"id\":77,\"run_attempt\":1,\"status\":\"completed\",\"conclusion\":\"success\"}}]}}"
    )
}

pub(crate) fn artifact_response_json() -> String {
    format!(
        "{{\"artifacts\":[{{\"name\":\"velnor-job-image-candidate-2\",\"id\":9,\"expired\":false,\"workflow_run\":{{\"id\":7,\"head_sha\":\"{SOURCE_SHA}\",\"head_branch\":\"main\"}}}}]}}"
    )
}

pub(crate) fn attestation_json() -> String {
    format!(
        "[{{\"verificationResult\":{{\"signature\":{{\"certificate\":{{\"sourceRepositoryIdentifier\":1390620900,\"githubWorkflowRepository\":\"tailrocks/velnor-new\",\"githubWorkflowSHA\":\"{SOURCE_SHA}\",\"githubWorkflowRef\":\"refs/heads/main\",\"githubWorkflowTrigger\":\"workflow_dispatch\",\"sourceRepositoryRef\":\"refs/heads/main\",\"sourceRepositoryDigest\":\"{SOURCE_SHA}\"}}}}}}}}]"
    )
}

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) driver: std::path::PathBuf,
}

pub(crate) fn write_bytes(root: &Path, name: &str, bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    let path = root.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path
        .to_str()
        .ok_or_else(|| fail("fixture path left its string shape"))?
        .to_owned())
}

pub(crate) fn stage_fixed_inputs(root: &Path) -> Result<std::path::PathBuf, Box<dyn Error>> {
    for (name, source) in [
        (IDENTITY_FILE, IDENTITY_SOURCE),
        (REGISTRY_FILE, REGISTRY_SOURCE),
        (PUBLISH_FILE, PUBLISH_SOURCE),
        (RECORD_FILE, RECORD_SOURCE),
        (OCI_FILE, OCI_SOURCE),
        (TAR_FILE, TAR_SOURCE),
    ] {
        std::fs::write(root.join(name), source)?;
    }
    let driver = root.join("gc1_t51_driver.py");
    std::fs::write(&driver, DRIVER)?;
    for name in [
        IDENTITY_FILE,
        REGISTRY_FILE,
        PUBLISH_FILE,
        RECORD_FILE,
        OCI_FILE,
        TAR_FILE,
    ] {
        let output = Command::new("python3")
            .arg("-m")
            .arg("py_compile")
            .arg(root.join(name))
            .output()?;
        if !output.status.success() {
            return Err(fail(&format!(
                "{name} left compilable:{:?}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
    }
    Ok(driver)
}

pub(crate) fn fixture() -> Result<Fixture, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let driver = stage_fixed_inputs(root.path())?;
    Ok(Fixture { root, driver })
}

pub(crate) fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
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

pub(crate) fn pins() -> ProductReleasePins {
    let setup = mise_setup();
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: argv(&["mise", "install", "gh@2.103.2"]),
        install_build_tools_argv: argv(&["mise", "install", "rust@1.91.0", "mr-boxington@1.0.0"]),
        install_qualify_tools_argv: argv(&["mise", "install", "opentofu@1.13.2"]),
        qualify_mbx_action_steps: [fixture_step(), fixture_step(), fixture_step()],
        install_runner_build_tools_argv: install_runner_build_tools_argv(),
        install_gh_argv: argv(&["mise", "install", "gh@2.103.2"]),
        build_argv: argv(&[
            "mbx",
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]),
        intel_build_argv: argv(&[
            "mbx",
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
            "--target",
            "x86_64-apple-darwin",
        ]),
        install_intel_target_argv: argv(&["rustup", "target", "add", "x86_64-apple-darwin"]),
        install_resource_probe_target_argv: install_resource_probe_target_argv(),
        runner_build_argv: runner_build_argv(),
        resource_probe_build_argv: resource_probe_build_argv(),
        install_resource_probe_release_tools_argv: install_resource_probe_release_tools_argv(),
        resource_probe_release_build_argv: resource_probe_release_build_argv(),
        resource_probe_release_test_argv: resource_probe_release_test_argv(),
        lint_argv: argv(&["bash", "-c", "exit 0"]),
        verify_lint_argv: verify_lint_argv(),
        gh_argv: argv(&["gh"]),
        rust_version: "1.91.0".to_owned(),
        verify_rust_argv: verify_rust_argv(),
        mr_boxington_version: "1.0.0".to_owned(),
        gh_version: "2.103.2".to_owned(),
    }
}

pub(crate) fn request() -> Result<Schema2WorkflowRequest, Box<dyn Error>> {
    Ok(Schema2WorkflowRequest {
        version: "0.1.55".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::ImageRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(pins()),
    })
}

pub(crate) fn candidate_text() -> Result<String, Box<dyn Error>> {
    let files = render_schema2_workflows(&request()?)?;
    let file = files
        .iter()
        .find(|file| file.path == CANDIDATE_PATH)
        .ok_or_else(|| fail("render lost its candidate"))?;
    Ok(file.bytes.clone())
}

#[path = "gc1_t51_identity.rs"]
mod gc1_t51_identity;
#[path = "gc1_t51_stream.rs"]
mod gc1_t51_stream;
#[path = "gc1_t51_trust.rs"]
mod gc1_t51_trust;
pub(crate) use gc1_t51_stream::*;

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn fixture_step() -> Step {
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

pub(crate) fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

pub(crate) fn verify_rust_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &format!("{} exit 0", credential_unset_prelude()),
    ])
}

pub(crate) fn verify_lint_argv() -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        &jactionlint_verifier_script("${VELNOR_TOOL_JACTIONLINT}", VERIFY_PATH),
    ])
}

pub(crate) fn install_runner_build_tools_argv() -> Vec<String> {
    argv(&["mise", "install", "rust@1.91.0"])
}

pub(crate) fn install_resource_probe_target_argv() -> Vec<String> {
    argv(&["rustup", "target", "add", "x86_64-unknown-linux-musl"])
}

pub(crate) fn runner_build_argv() -> Vec<String> {
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

pub(crate) fn attestation_helper_build_argv() -> Vec<String> {
    argv(&[
        "mbx",
        "build",
        "--locked",
        "--manifest-path",
        "crates/velnor-runner/Cargo.toml",
        "--release",
        "--package",
        "velnor-runner-attestation",
        "--bin",
        "velnor-runner-attestation-helper",
    ])
}

pub(crate) fn resource_probe_build_argv() -> Vec<String> {
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

pub(crate) fn install_resource_probe_release_tools_argv() -> Vec<String> {
    argv(&["mise", "install", "rust@1.91.0", "mr-boxington@1.0.0"])
}

pub(crate) fn resource_probe_release_build_argv() -> Vec<String> {
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

pub(crate) fn resource_probe_release_test_argv() -> Vec<String> {
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
        attestation_helper_build_argv: attestation_helper_build_argv(),
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

pub(crate) fn find_file<'a>(
    files: &'a [RenderedFile],
    path: &str,
) -> Result<&'a str, Box<dyn Error>> {
    files
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.bytes.as_str())
        .ok_or_else(|| fail(&format!("render lost its file:{path}")))
}

pub(crate) fn candidate_text() -> Result<String, Box<dyn Error>> {
    let files = render_schema2_workflows(&request()?)?;
    Ok(find_file(&files, CANDIDATE_PATH)?.to_owned())
}

pub(crate) fn check_body_composition() -> Result<(), Box<dyn Error>> {
    for (name, fragment) in [
        (
            "identity fragment",
            "include_str!(\"job_image_authority_identity.py\")",
        ),
        (
            "registry fragment",
            "include_str!(\"job_image_authority_registry.py\")",
        ),
        (
            "publication fragment",
            "include_str!(\"job_image_authority_publish.py\")",
        ),
        (
            "record fragment",
            "include_str!(\"job_image_authority_record.py\")",
        ),
        (
            "oci fragment",
            "include_str!(\"job_image_authority_oci.py\")",
        ),
        (
            "tar fragment",
            "include_str!(\"job_image_authority_tar.py\")",
        ),
    ] {
        require(AUTHORITY_MODULE, fragment, name)?;
    }
    require_count(AUTHORITY_MODULE, "include_str!", 6, "authority body")?;
    require(AUTHORITY_MODULE, "fn materialize_step()", "authority body")?;
    for marker in [
        "Materialize fixed image authority implementation",
        "$RUNNER_TEMP/velnor-image-authority",
        "VELNOR_IMAGE_AUTHORITY_EOF",
    ] {
        require(AUTHORITY_MODULE, marker, "authority body")?;
    }
    for kept in [
        "AuthorityMode",
        "AttestationMode",
        "PublicationMode",
        "candidate.py",
        "images/job/ubuntu-26.04/candidate/publish",
        "image-maintenance",
        "process::Command",
        "subprocess",
    ] {
        forbid(AUTHORITY_MODULE, kept, "authority body")?;
    }
    Ok(())
}

pub(crate) fn check_publish_job_pushes_before_evidence(text: &str) -> Result<(), Box<dyn Error>> {
    for (marker, label) in [
        (
            "Assemble publication evidence with the fixed authority body",
            "evidence step",
        ),
        ("VELNOR_REGISTRY_TOKEN", "registry token"),
        ("fetch_registry_token", "registry token exchange"),
        ("publish_image", "registry publication"),
        ("verify_remote_manifest", "registry readback"),
        ("check_publication_proof", "publication proof"),
    ] {
        require(text, marker, label)?;
    }
    Ok(())
}

pub(crate) fn check_modes_share_body(text: &str) -> Result<(), Box<dyn Error>> {
    require(text, "attest-image-candidate", "attestation mode")?;
    require(text, "publish-job-image", "publication mode")?;
    require_count(text, STEP_NAME, 2, "shared body step")?;
    require_count(text, BODY_DIRECTORY, 2, "shared body directory")?;
    require_count(text, END_MARKER, 24, "shared body files")?;
    for (file, label) in [
        (IDENTITY_FILE, "identity file"),
        (REGISTRY_FILE, "registry file"),
        (PUBLISH_FILE, "publication file"),
        (RECORD_FILE, "record file"),
        (OCI_FILE, "oci file"),
        (TAR_FILE, "tar file"),
    ] {
        require_count(text, file, 2, label)?;
    }
    for (marker, label) in [
        ("REPOSITORY = ", "identity head"),
        ("def validate_build_lock", "identity tail"),
        ("REGISTRY = ", "registry head"),
        ("def redirect_target", "registry tail"),
        ("EVIDENCE_SCHEMA = ", "publication head"),
        ("def validate_evidence", "publication tail"),
        ("INPUT_PATHS = (", "record head"),
        ("RECORD_KEYS = {", "record tail"),
        ("LAYER_MEDIA_TYPES = {", "oci head"),
        ("def verify_oci_archive", "oci tail"),
    ] {
        require_count(text, marker, 2, label)?;
    }
    Ok(())
}

pub(crate) struct Fixture {
    pub(crate) root: tempfile::TempDir,
    pub(crate) driver: std::path::PathBuf,
}

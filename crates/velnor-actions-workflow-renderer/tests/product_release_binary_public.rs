// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::process::Output;

#[path = "product_release_binary_fixture.rs"]
mod product_release_binary_fixture;

#[test]
fn binary_publisher_uses_linux_setup_while_build_remains_macos()
-> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeSet;
    use velnor_actions_contract::RoutingWorkflow;
    use velnor_actions_workflow_renderer::schema2::{
        Schema2WorkflowRequest, render_schema2_workflows,
    };
    let mut pins = product_release_binary_fixture::pins();
    pins.linux_x86_64_setup.sha256 = "a".repeat(64);
    pins.macos_arm64_setup.sha256 = "b".repeat(64);
    let request = Schema2WorkflowRequest {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::MacosBinaryRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(pins),
    };
    let files = render_schema2_workflows(&request)?;
    let workflow = files
        .iter()
        .find(|file| file.path == ".github/workflows/product-release-binary.yml")
        .ok_or("missing workflow")?;
    let body = &workflow.bytes;
    let build = body
        .split("  build-binary:")
        .nth(1)
        .ok_or("missing build job")?
        .split("\n  attest-binary:")
        .next()
        .ok_or("missing build end")?;
    let publish = body
        .split("  publish-binary:")
        .nth(1)
        .ok_or("missing publish job")?;
    let attest = body
        .split("  attest-binary:")
        .nth(1)
        .ok_or("missing attest job")?
        .split("\n  publish-binary:")
        .next()
        .ok_or("missing attest end")?;
    assert!(build.contains("macos-15"));
    assert!(attest.contains("macos-15"));
    assert!(publish.contains("ubuntu-26.04"));
    assert!(publish.contains(&"a".repeat(64)));
    assert!(!publish.contains(&"b".repeat(64)));
    Ok(())
}

#[test]
fn release_authority_jobs_use_qualified_linux_for_all_hosted_labels()
-> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeSet;
    use velnor_actions_contract::RoutingWorkflow;
    use velnor_actions_workflow_renderer::schema2::{
        Schema2WorkflowRequest, render_schema2_workflows,
    };
    for label in ["ubuntu-26.04", "ubuntu-26.04-arm"] {
        let request = Schema2WorkflowRequest {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            hosted_label: label.to_owned(),
            scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
            workflows: BTreeSet::from([
                RoutingWorkflow::ImageRelease,
                RoutingWorkflow::GeneratorRelease,
            ]),
            mbx_qualification: None,
            mise_pin_qualification: None,
            product_release: Some(product_release_binary_fixture::pins()),
        };
        let files = render_schema2_workflows(&request)?;
        for (path, jobs) in [
            (
                ".github/workflows/product-release-images.yml",
                &["publish-images"][..],
            ),
            (
                ".github/workflows/product-release-generator.yml",
                &["publish-generator"][..],
            ),
            (
                ".github/workflows/job-image-candidate.yml",
                &["attest-image-candidate", "publish-job-image"][..],
            ),
        ] {
            let file = files
                .iter()
                .find(|file| file.path == path)
                .ok_or("missing workflow")?;
            let workflow: serde_json::Value = serde_saphyr::from_str(&file.bytes)?;
            for job in jobs {
                assert_eq!(
                    workflow["jobs"][*job]["runs-on"], "ubuntu-26.04",
                    "{path}:{job} for {label}"
                );
            }
        }
    }
    Ok(())
}
use product_release_binary_fixture::{BinaryReleaseFixture, BuildMode, HELPER_SUFFIX, SOURCE_SHA};
#[path = "product_release_binary_hash_fixture.rs"]
mod product_release_binary_hash_fixture;
use product_release_binary_hash_fixture::{macho_bytes, sha256};

const HELPER_NAME: &str = "velnor-runner-attestation-helper";
const HOST_NAME: &str = "velnor-host";
const TARGET_HELPER_PATH: &str =
    "crates/velnor-runner/target/release/velnor-runner-attestation-helper";

#[test]
fn binary_release_packages_helper_bound_to_host_and_checksums() -> Result<(), Box<dyn Error>> {
    let fixture = BinaryReleaseFixture::new(BuildMode::Success)?;
    let build = fixture.execute_build(Some(SOURCE_SHA))?;
    let expected_helper = macho_bytes(HELPER_SUFFIX);
    let helper = fs::read(fixture.workspace().join(HELPER_NAME)).unwrap_or_default();
    assert_eq!(
        helper,
        expected_helper,
        "packaged helper bytes are absent or incorrect; {}",
        output_text(&build)
    );
    assert!(build.status.success(), "{}", output_text(&build));
    let helper_sha = sha256(&helper);
    let host = macho_bytes(format!("source={SOURCE_SHA}\nhelper={helper_sha}\n").as_bytes());
    let packaged_host = fs::read(fixture.workspace().join(HOST_NAME)).unwrap_or_default();
    assert_eq!(
        packaged_host, host,
        "host artifact receipt does not bind the exact source SHA to the packaged helper hash"
    );

    let verify = fixture.execute_verify()?;
    assert!(verify.status.success(), "{}", output_text(&verify));
    let checksum = fixture.execute_checksum()?;
    assert!(checksum.status.success(), "{}", output_text(&checksum));

    let license = fs::read(fixture.workspace().join("LICENSE"))?;
    let notice = fs::read(fixture.workspace().join("NOTICE"))?;
    let expected = [
        checksum_line(HOST_NAME, &host),
        checksum_line(HELPER_NAME, &helper),
        checksum_line("LICENSE", &license),
        checksum_line("NOTICE", &notice),
    ]
    .concat();
    assert_eq!(
        fs::read_to_string(fixture.workspace().join("SHA256SUMS"))?,
        expected
    );
    Ok(())
}

#[test]
fn binary_release_rejects_helper_build_failure() -> Result<(), Box<dyn Error>> {
    assert_rejected(BuildMode::HelperFailure, Some(SOURCE_SHA))
}

#[test]
fn binary_release_rejects_missing_helper_before_packaging() -> Result<(), Box<dyn Error>> {
    let fixture = BinaryReleaseFixture::new(BuildMode::MissingHelper)?;
    let output = fixture.execute_build(Some(SOURCE_SHA))?;
    assert!(
        !output.status.success(),
        "packaging unexpectedly succeeded without helper: {}",
        output_text(&output)
    );
    assert!(!fixture.workspace().join(HOST_NAME).exists());
    assert!(!fixture.workspace().join("SHA256SUMS").exists());
    Ok(())
}

#[test]
fn binary_release_rejects_host_build_failure() -> Result<(), Box<dyn Error>> {
    assert_rejected(BuildMode::HostFailure, Some(SOURCE_SHA))
}

#[test]
fn binary_release_rejects_helper_changed_before_copy() -> Result<(), Box<dyn Error>> {
    assert_rejected(BuildMode::TamperedHelper, Some(SOURCE_SHA))
}

#[test]
fn binary_release_rejects_initial_helper_hash_failure() -> Result<(), Box<dyn Error>> {
    let fixture = BinaryReleaseFixture::new(BuildMode::InitialHelperHashFailure)?;
    let output = fixture.execute_build(Some(SOURCE_SHA))?;
    assert!(
        !output.status.success(),
        "packaging unexpectedly succeeded after the initial helper hash failed: {}",
        output_text(&output)
    );
    assert!(!fixture.workspace().join(HOST_NAME).exists());
    assert!(!fixture.workspace().join("SHA256SUMS").exists());
    let helper = fs::read(fixture.workspace().join(TARGET_HELPER_PATH))?;
    assert_eq!(
        fixture.shasum_receipt()?,
        expected_hash_failure_receipt("initial-helper", TARGET_HELPER_PATH, &helper)
    );
    Ok(())
}

#[test]
fn binary_release_rejects_copied_helper_hash_failure() -> Result<(), Box<dyn Error>> {
    let fixture = BinaryReleaseFixture::new(BuildMode::CopiedHelperHashFailure)?;
    let output = fixture.execute_build(Some(SOURCE_SHA))?;
    assert!(
        !output.status.success(),
        "packaging unexpectedly succeeded after the copied helper hash failed: {}",
        output_text(&output)
    );
    assert!(fixture.workspace().join(HOST_NAME).is_file());
    assert!(!fixture.workspace().join("SHA256SUMS").exists());
    let helper = fs::read(fixture.workspace().join(HELPER_NAME))?;
    assert_eq!(
        fixture.shasum_receipt()?,
        expected_hash_failure_receipt("copied-helper", HELPER_NAME, &helper)
    );
    Ok(())
}

#[test]
fn binary_release_requires_source_sha_before_building() -> Result<(), Box<dyn Error>> {
    assert_rejected(BuildMode::MissingSource, None)
}

fn assert_rejected(mode: BuildMode, source_sha: Option<&str>) -> Result<(), Box<dyn Error>> {
    let fixture = BinaryReleaseFixture::new(mode)?;
    let output = fixture.execute_build(source_sha)?;
    assert!(
        !output.status.success(),
        "packaging unexpectedly succeeded in a failure scenario: {}",
        output_text(&output)
    );
    assert!(!fixture.workspace().join("SHA256SUMS").exists());
    Ok(())
}

fn checksum_line(name: &str, bytes: &[u8]) -> String {
    format!("{}  {name}\n", sha256(bytes))
}

fn expected_hash_failure_receipt(stage: &str, path: &str, bytes: &[u8]) -> String {
    format!(
        "stage={stage} real_status=0 emitted={}  {path} forced_status=43\n",
        sha256(bytes)
    )
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

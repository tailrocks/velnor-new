// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn image_release_publishes_only_a_jointly_signed_digest_manifest() -> Result<(), Box<dyn Error>> {
    let files = render_schema2_workflows(&request()?)?;
    let caller = find_file(&files, ".github/workflows/runner-image-release.yml")?;
    assert!(caller.contains("push:") && caller.contains("- main"));
    assert!(caller.contains("./.github/workflows/product-release-images.yml"));
    assert!(!caller.contains("release-generator") && !caller.contains("release-binary"));
    let workflow = find_file(&files, ".github/workflows/product-release-images.yml")?;
    let build = job_section(workflow, "build-images:", Some("attest-images:"))?;
    for command in [
        "admit-runner-images",
        "write-runner-image-manifest",
        "validate_digest_attestation",
        "Acquire independently verified exact-source SDK preview",
    ] {
        assert!(build.contains(command), "missing {command}");
    }
    let signer = job_section(workflow, "attest-images:", Some("publish-images:"))?;
    assert_eq!(signer.matches(ATTEST_BUILD_PROVENANCE_USES).count(), 1);
    for asset in [
        "RUNNER_IMAGE_MANIFEST.json",
        "SHA256SUMS",
        "LICENSE",
        "NOTICE",
    ] {
        assert!(signer.contains(&format!("assets/{asset}")));
    }
    let publisher = job_section(workflow, "publish-images:", None)?;
    for job in [signer, publisher] {
        assert!(!job.contains(CHECKOUT_USES));
        assert!(!job.contains("cargo ") && !job.contains("mbx "));
    }
    assert!(!workflow.contains("velnor-runner-linux-amd64.tar"));
    assert!(!workflow.contains("RESOURCE_PROBE_MANIFEST.json"));
    assert!(workflow.contains("runner-image-release.yml@refs/heads/main/push"));
    Ok(())
}

#[test]
fn runner_manifest_requires_matching_source_publication_and_verified_digest()
-> Result<(), Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("python3")
        .arg(root.join("scripts/test_runner_image_publication_policy.py"))
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
#[test]
#[cfg(unix)]
fn release_consumers_keep_the_build_artifact_identity_on_job_reruns() -> Result<(), Box<dyn Error>>
{
    let files = render_schema2_workflows(&request()?)?;
    let workflow = find_file(&files, ".github/workflows/product-release-images.yml")?;
    let build = job_section(workflow, "build-images:", Some("attest-images:"))?;
    assert!(build.contains("id: upload-assets"));
    assert!(build.contains("name: image-assets-${{ github.run_attempt }}"));
    assert!(build.contains("artifact_id: ${{ steps.upload-assets.outputs.artifact-id }}"));

    let attest = job_section(workflow, "attest-images:", Some("publish-images:"))?;
    assert!(attest.contains("- build-images"));
    assert!(attest.contains("ARTIFACT_ID: ${{ needs.build-images.outputs.artifact_id }}"));
    assert!(attest.contains("artifact-ids: ${{ env.ARTIFACT_ID }}"));
    assert!(attest.contains("artifact_id: ${{ needs.build-images.outputs.artifact_id }}"));

    let publish = job_section(workflow, "publish-images:", None)?;
    assert!(publish.contains("- attest-images"));
    assert!(publish.contains("ARTIFACT_ID: ${{ needs.attest-images.outputs.artifact_id }}"));
    assert!(publish.contains("artifact-ids: ${{ env.ARTIFACT_ID }}"));
    assert_eq!(
        build
            .matches("name: image-assets-${{ github.run_attempt }}")
            .count(),
        1
    );
    assert_download_guard(workflow, "attest-images")?;
    assert_download_guard(workflow, "publish-images")?;
    assert_build_only_artifact_steps(workflow, "publish-images")?;

    let binary_request = Schema2WorkflowRequest {
        workflows: BTreeSet::from([RoutingWorkflow::MacosBinaryRelease]),
        ..request()?
    };
    let binary_files = render_schema2_workflows(&binary_request)?;
    let binary = find_file(
        &binary_files,
        ".github/workflows/product-release-binary.yml",
    )?;
    let build = job_section(binary, "build-binary:", Some("attest-binary:"))?;
    assert!(build.contains("name: binary-assets-${{ github.run_attempt }}"));
    assert!(build.contains("artifact_id: ${{ steps.upload-assets.outputs.artifact-id }}"));
    let attest = job_section(binary, "attest-binary:", Some("publish-binary:"))?;
    assert!(attest.contains("ARTIFACT_ID: ${{ needs.build-binary.outputs.artifact_id }}"));
    assert!(attest.contains("artifact-ids: ${{ env.ARTIFACT_ID }}"));
    assert!(attest.contains("artifact_id: ${{ needs.build-binary.outputs.artifact_id }}"));
    let publish = job_section(binary, "publish-binary:", None)?;
    assert!(publish.contains("ARTIFACT_ID: ${{ needs.attest-binary.outputs.artifact_id }}"));
    assert!(publish.contains("artifact-ids: ${{ env.ARTIFACT_ID }}"));
    assert_eq!(binary.matches("github.run_attempt").count(), 1);
    assert_download_guard(binary, "attest-binary")?;
    assert_download_guard(binary, "publish-binary")?;
    assert_build_only_artifact_steps(binary, "publish-binary")?;
    Ok(())
}

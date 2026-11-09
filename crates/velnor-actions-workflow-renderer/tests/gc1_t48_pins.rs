// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

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
        runner_image_family: None,
        version: "0.1.55".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::ImageRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(pins()),
    })
}

#[cfg(unix)]
#[test]
fn image_release_builds_both_images_with_the_approved_source_revision() -> Result<(), Box<dyn Error>>
{
    use std::os::unix::fs::PermissionsExt;

    let files = render_schema2_workflows(&request()?)?;
    let workflow = find_file(&files, ".github/workflows/product-release-images.yml")?;
    let step = workflow
        .split("- name: Build runner and DinD images")
        .nth(1)
        .and_then(|tail| tail.split("- name: ").next())
        .ok_or_else(|| fail("image build step missing"))?;
    assert!(step.contains("VELNOR_SOURCE_SHA: ${{ inputs.source_sha }}"));
    let encoded = step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or_else(|| fail("image build command missing"))?;
    let script: String = serde_json::from_str(encoded)?;
    let dir = tempfile::tempdir()?;
    let docker = dir.path().join("docker");
    let log = dir.path().join("builds");
    std::fs::write(
        &docker,
        "#!/bin/sh\nif [ \"$1\" = build ]; then printf '%s\\n' \"$*\" >> \"$BUILD_LOG\"; elif [ \"$4\" = '{{.Architecture}}' ]; then printf '%s\\n' amd64; elif [ \"$5\" = \"${BAD_IMAGE:-}\" ]; then printf '%s\\n' wrong; else printf '%s\\n' \"$VELNOR_SOURCE_SHA\"; fi\n",
    )?;
    std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o700))?;
    let source = "a97f3c482da31b6f91f83e28c4dca0217e5b6930";
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .env("PATH", dir.path())
        .env("BUILD_LOG", &log)
        .env("VELNOR_SOURCE_SHA", source)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let builds = std::fs::read_to_string(log)?;
    for context in ["images/runner/ubuntu-26.04", "images/dind"] {
        assert!(
            builds.lines().any(|line| line
                .contains(&format!("--build-arg VELNOR_SOURCE_SHA={source}"))
                && line.ends_with(context)),
            "missing source revision for {context}: {builds}"
        );
    }
    let missing_log = dir.path().join("missing-source-builds");
    assert!(
        !Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .env("PATH", dir.path())
            .env("BUILD_LOG", &missing_log)
            .env_remove("VELNOR_SOURCE_SHA")
            .output()?
            .status
            .success()
    );
    assert!(!missing_log.exists());
    verify_revisions(workflow, dir.path(), source)
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
    assert_eq!(workflow.matches("github.run_attempt").count(), 1);
    assert_download_guard(workflow, "attest-images")?;
    assert_download_guard(workflow, "publish-images")?;
    assert_build_only_artifact_steps(workflow, "publish-images")?;

    let binary_request = Schema2WorkflowRequest {
        runner_image_family: None,
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

#[cfg(unix)]
fn assert_download_guard(workflow: &str, job: &str) -> Result<(), Box<dyn Error>> {
    let section = job_section(workflow, &format!("{job}:"), None)?;
    let guard_marker = "- name: Validate producer artifact identity";
    let guard = section
        .split(guard_marker)
        .nth(1)
        .and_then(|tail| tail.split("- name: ").next())
        .ok_or_else(|| fail("artifact identity guard missing"))?;
    let download = section
        .find("- name: Download built assets")
        .ok_or_else(|| fail("artifact download missing"))?;
    let guard_index = section
        .find(guard_marker)
        .ok_or_else(|| fail("artifact identity guard missing"))?;
    assert!(
        guard_index < download,
        "guard must precede artifact download"
    );
    assert!(guard.contains("ARTIFACT_ID: ${{ needs."));
    let encoded = guard
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or_else(|| fail("artifact identity guard command missing"))?;
    let script: String = serde_json::from_str(encoded)?;
    for (id, expected) in [
        ("1", true),
        ("9007199254740991", true),
        ("", false),
        ("0", false),
        ("01", false),
        ("1x", false),
        ("9007199254740992", false),
    ] {
        let result = Command::new("/bin/bash")
            .arg("-c")
            .arg(&script)
            .env("ARTIFACT_ID", id)
            .output()?;
        assert_eq!(
            result.status.success(),
            expected,
            "artifact identity guard result for {id:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let download_section = &section[download..];
    assert!(download_section.contains("ARTIFACT_ID: ${{ needs."));
    assert!(download_section.contains("artifact-ids: ${{ env.ARTIFACT_ID }}"));
    Ok(())
}

#[cfg(unix)]
fn assert_build_only_artifact_steps(workflow: &str, job: &str) -> Result<(), Box<dyn Error>> {
    let section = job_section(workflow, &format!("{job}:"), None)?;
    for marker in [
        "- name: Validate producer artifact identity",
        "- name: Download built assets",
    ] {
        let step = section
            .split(marker)
            .nth(1)
            .and_then(|tail| tail.split("- name: ").next())
            .ok_or_else(|| fail("build-only release step missing"))?;
        assert!(
            step.contains("if: inputs.release_action == 'build'"),
            "complete release must skip {marker}"
        );
    }
    Ok(())
}

#[cfg(unix)]
fn verify_revisions(workflow: &str, bin: &Path, source: &str) -> Result<(), Box<dyn Error>> {
    let verify_marker = "- name: Verify image architectures and revisions";
    let verify_start = workflow
        .find(verify_marker)
        .ok_or_else(|| fail("image verification step missing"))?;
    let save_start = workflow
        .find("- name: Save image archives")
        .ok_or_else(|| fail("image save step missing"))?;
    assert!(verify_start < save_start);
    let verify_step = &workflow[verify_start..save_start];
    assert!(verify_step.contains("VELNOR_SOURCE_SHA: ${{ inputs.source_sha }}"));
    let encoded = verify_step
        .lines()
        .find_map(|line| line.trim_start().strip_prefix("run: "))
        .ok_or_else(|| fail("image verification command missing"))?;
    let verify_script: String = serde_json::from_str(encoded)?;
    for bad_image in ["", "velnor-runner:linux-amd64", "velnor-dind:linux-amd64"] {
        let result = Command::new("/bin/sh")
            .arg("-c")
            .arg(&verify_script)
            .env("PATH", bin)
            .env("VELNOR_SOURCE_SHA", source)
            .env("BAD_IMAGE", bad_image)
            .output()?;
        assert_eq!(
            result.status.success(),
            bad_image.is_empty(),
            "verification result for {bad_image}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
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

pub(crate) fn job_section<'a>(
    text: &'a str,
    start: &str,
    end: Option<&str>,
) -> Result<&'a str, Box<dyn Error>> {
    let begin = text
        .find(start)
        .ok_or_else(|| fail(&format!("render lost its job:{start}")))?;
    let tail = &text[begin..];
    match end {
        Some(marker) => {
            let close = tail
                .find(marker)
                .ok_or_else(|| fail(&format!("render lost its job:{marker}")))?;
            Ok(&tail[..close])
        }
        None => Ok(tail),
    }
}

pub(crate) const SOURCE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(crate) const WORKFLOW_REF: &str =
    "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml@refs/heads/main";

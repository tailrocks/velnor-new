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
        version: "0.1.55".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::ImageRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(pins()),
    })
}

#[path = "gc1_t48_runner_images.rs"]
mod runner_images;

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

#[test]
fn runner_qualification_signs_only_the_exact_credential_free_runtime_proof()
-> Result<(), Box<dyn Error>> {
    let files = render_schema2_workflows(&request()?)?;
    let caller = find_file(&files, ".github/workflows/runner-image-qualification.yml")?;
    let signer = find_file(
        &files,
        ".github/workflows/runner-image-qualification-attest.yml",
    )?;
    assert!(caller.contains("group: ${{ github.workflow }}-caller-${{ github.ref }}"));
    assert!(caller.contains("cancel-in-progress: true"));
    assert!(!signer.contains("-caller-${{ github.ref }}"));
    let qualify = job_section(caller, "qualify-images:", Some("attest:"))?;
    let install = qualify
        .find("Install pinned resource probe tools")
        .ok_or_else(|| fail("missing install"))?;
    let verified = qualify
        .find("Verify Rust channel manifest")
        .ok_or_else(|| fail("missing verifier"))?;
    for name in [
        "Build exact-source image maintenance",
        "Build exact-source runtime qualifier",
    ] {
        let build = qualify.find(name).ok_or_else(|| fail("missing build"))?;
        assert!(install < verified && verified < build);
    }
    let permissions = qualify
        .split("steps:")
        .next()
        .ok_or_else(|| fail("missing steps"))?;
    assert!(!permissions.contains("id-token"));
    assert!(!permissions.contains("attestations"));
    assert!(!permissions.contains("write"));
    for expected in [
        "refs/heads/main",
        "Fetch exact successful CI candidates",
        "qualify-runner-images",
        "write-runner-qualification",
        "PRODUCER_RUN_ID",
        "QUALIFIER_JOB_ID",
        "*TOKEN*",
        "*PASSWORD*",
        "*SECRET*",
        "RUNNER_IMAGE_QUALIFICATION.json",
    ] {
        assert!(qualify.contains(expected), "missing {expected}");
    }
    for role in ["runner.oci.tar", "dind.oci.tar", "resource-probe.oci.tar"] {
        assert!(qualify.contains(role), "missing {role}");
    }
    assert!(caller.contains("./.github/workflows/runner-image-qualification-attest.yml"));
    assert!(caller.contains("needs.qualify-images.outputs.artifact_id"));
    assert!(signer.contains("inputs.artifact_id"));
    assert!(signer.contains("RUNNER_IMAGE_QUALIFICATION.json"));
    assert!(signer.contains(ATTEST_BUILD_PROVENANCE_USES));
    assert!(!signer.contains(CHECKOUT_USES));
    assert!(!signer.contains("run:"));
    Ok(())
}

#[test]
fn ci_job_image_attestor_binds_three_exact_subjects_before_signing() -> Result<(), Box<dyn Error>> {
    let workflow = candidate_text()?;
    assert!(workflow.contains("group: ${{ github.workflow }}-${{ github.ref }}"));
    assert!(workflow.contains("cancel-in-progress: true"));
    let mut disabled = request()?;
    disabled.workflows.clear();
    assert!(
        !render_schema2_workflows(&disabled)?
            .iter()
            .any(|file| file.path == CANDIDATE_PATH)
    );
    let mut unpinned = request()?;
    unpinned.product_release = None;
    assert!(render_schema2_workflows(&unpinned).is_err());
    for expected in [
        "Fetch exact successful CI candidates",
        "validate_required",
        "job-image.oci.tar",
        "job-image-candidate.json",
        "job-image-native-source-closure.json",
        "record_module.check_record_binding",
        "record_module.validate_ci_candidate",
        "Verify candidate bytes with the fixed authority body",
        ATTEST_BUILD_PROVENANCE_USES,
    ] {
        assert!(workflow.contains(expected), "missing {expected}");
    }
    assert!(
        workflow.find("Verify candidate bytes with the fixed authority body")
            < workflow.find(ATTEST_BUILD_PROVENANCE_USES)
    );
    assert!(!workflow.contains(CHECKOUT_USES));
    assert!(!workflow.contains("publish-job-image"));
    assert!(!workflow.contains("job_image_authority_publish.py"));
    Ok(())
}

#[test]
fn ci_job_attestor_checks_real_archive_and_source_before_signing() -> Result<(), Box<dyn Error>> {
    let text = candidate_text()?;
    let verify = authority_script(
        &text,
        "Verify candidate bytes with the fixed authority body",
    )?;
    let root = tempfile::tempdir()?;
    let candidate = root.path().join("privatecandidate");
    std::fs::create_dir(&candidate)?;
    materialize_authority(&text, root.path())?;
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let archive_path = candidate.join("job-image.oci.tar");
    let record_path = candidate.join("job-image-candidate.json");
    let closure_path = candidate.join("job-image-native-source-closure.json");
    let mut record: serde_json::Value = serde_json::from_str(&record_json(&parts, &archive))?;
    record["workflow"]["repository_id"] = serde_json::json!(1_390_620_900);
    record["workflow"]["workflow_ref"] =
        serde_json::json!("tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main");
    record["workflow"]["authority_commit"] = serde_json::json!(SOURCE_SHA);
    std::fs::write(&archive_path, &archive)?;
    std::fs::write(&record_path, serde_json::to_vec(&record)?)?;
    std::fs::write(
        &closure_path,
        serde_json::to_vec(
            &serde_json::json!({"schema":"velnor.job-image-native-source-closure.v1", "source_commit": SOURCE_SHA}),
        )?,
    )?;
    let run = || {
        Command::new("bash")
            .arg("-c")
            .arg(&verify)
            .env("RUNNER_TEMP", root.path())
            .env(
                "VELNOR_IMAGE_AUTHORITY_DIR",
                root.path().join("velnor-image-authority"),
            )
            .env("VELNOR_EXPECTED_SOURCE", SOURCE_SHA)
            .env("VELNOR_EXPECTED_REF", "refs/heads/main")
            .env("VELNOR_EXPECTED_REPOSITORY", "tailrocks/velnor-new")
            .env("VELNOR_EXPECTED_EVENT", "push")
            .env(
                "VELNOR_EXPECTED_WORKFLOW_REF",
                "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
            )
            .env("VELNOR_EXPECTED_RUN_ID", "1")
            .env("VELNOR_EXPECTED_RUN_ATTEMPT", "1")
            .output()
    };
    let valid = run()?;
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    record["workflow"]["repository_id"] = serde_json::json!(1);
    std::fs::write(&record_path, serde_json::to_vec(&record)?)?;
    assert!(!run()?.status.success());
    record["workflow"]["repository_id"] = serde_json::json!(1_390_620_900);
    std::fs::write(&record_path, serde_json::to_vec(&record)?)?;
    std::fs::write(&archive_path, b"corrupt")?;
    assert!(!run()?.status.success());
    std::fs::write(&archive_path, &archive)?;
    record["workflow"]["run_attempt"] = serde_json::json!(2);
    std::fs::write(&record_path, serde_json::to_vec(&record)?)?;
    assert!(!run()?.status.success());
    record["workflow"]["run_attempt"] = serde_json::json!(1);
    std::fs::write(&record_path, serde_json::to_vec(&record)?)?;
    std::fs::write(
        &closure_path,
        br#"{"schema":"velnor.job-image-native-source-closure.v1","source_commit":"wrong"}"#,
    )?;
    assert!(!run()?.status.success());
    Ok(())
}

fn authority_script(text: &str, name: &str) -> Result<String, Box<dyn Error>> {
    let section = text.split_once(name).ok_or("step")?.1;
    let line = section
        .lines()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or("run")?;
    Ok(serde_json::from_str(line)?)
}

fn materialize_authority(text: &str, root: &std::path::Path) -> Result<(), Box<dyn Error>> {
    let output = Command::new("bash")
        .arg("-c")
        .arg(authority_script(
            text,
            "Materialize fixed image authority implementation",
        )?)
        .env("RUNNER_TEMP", root)
        .env("GITHUB_ENV", root.join("github-env"))
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn runner_qualification_requires_exact_receipt_and_source_free_signer() -> Result<(), Box<dyn Error>>
{
    let mut request = request()?;
    request.runner_image_family = true;
    let files = render_schema2_workflows(&request)?;
    let caller = find_file(&files, ".github/workflows/runner-image-qualification.yml")?;
    let signer = find_file(
        &files,
        ".github/workflows/runner-image-qualification-attest.yml",
    )?;
    let caller_header = caller
        .split("jobs:")
        .next()
        .ok_or("qualification caller header missing")?;
    assert!(caller_header.contains("runner-image-qualification-caller-${{ github.ref }}"));
    assert!(caller_header.contains("cancel-in-progress: true"));
    assert!(!caller_header.contains("${{ github.repository }}-product-release"));
    let publisher_header = find_file(&files, ".github/workflows/product-release-images.yml")?
        .split("jobs:")
        .next()
        .ok_or("runner image release header missing")?;
    assert!(publisher_header.contains("${{ github.repository }}-product-release"));
    assert!(publisher_header.contains("cancel-in-progress: false"));
    let qualifier = job_section(caller, "  qualify-images:", Some("  attest-qualification:"))?;
    let attestor = job_section(caller, "  attest-qualification:", None)?;
    assert!(qualifier.contains("resolve-runner-family-producer.sh"));
    assert!(qualifier.contains("qualify-runner-image-archives"));
    assert!(qualifier.contains("steps.identity.outputs.producer_artifact_id"));
    assert!(qualifier.contains("steps.identity.outputs.producer_run_id"));
    assert!(qualifier.contains("steps.identity.outputs.qualifier_job_id"));
    assert!(qualifier.contains("runner-image-publication.json"));
    assert!(qualifier.contains("dind-image-publication.json"));
    assert!(qualifier.contains("resource-probe-image-publication.json"));
    for name in [
        "runner-image-provenance.json",
        "dind-image-provenance.json",
        "resource-probe-image-provenance.json",
    ] {
        assert!(qualifier.contains(name), "qualifier omits {name}");
    }
    assert!(qualifier.contains(r#"$assets\" \"$assets/runner-image-publication.json"#));
    assert!(!qualifier.contains("id-token: write"));
    assert!(!qualifier.contains("attestations: write"));
    assert!(attestor.contains("qualify-images"));
    assert!(attestor.contains("needs.qualify-images.outputs.receipt_artifact_id"));
    assert!(attestor.contains("runner-image-qualification-attest.yml"));
    assert!(signer.contains("inputs.receipt_artifact_id"));
    assert!(signer.contains("RUNNER_IMAGE_QUALIFICATION.json"));
    assert!(signer.contains(ATTEST_BUILD_PROVENANCE_USES));
    assert!(!signer.contains(CHECKOUT_USES));
    Ok(())
}

#[cfg(unix)]
#[test]
fn runner_qualification_discovers_unique_actual_run_artifact_and_job() -> Result<(), Box<dyn Error>>
{
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};

    let dir = tempfile::tempdir()?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gh = dir.path().join("gh");
    fs::write(
        &gh,
        "#!/bin/sh\ncase \"$*\" in *workflows/ci.yml/runs*) cat \"$FIXTURE/runs\";; *actions/runs/31/artifacts*) cat \"$FIXTURE/artifacts\";; *actions/runs/41/attempts/2/jobs*) cat \"$FIXTURE/jobs\";; *) exit 1;; esac\n",
    )?;
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o700))?;
    let sha = "a".repeat(40);
    fs::write(
        dir.path().join("runs"),
        format!(
            r#"[{{"total_count":1,"workflow_runs":[{{"path":".github/workflows/ci.yml","event":"push","head_branch":"main","head_sha":"{sha}","head_repository":{{"full_name":"tailrocks/velnor-new"}},"id":31,"run_number":7,"run_attempt":2,"status":"completed","conclusion":"success"}}]}}]"#
        ),
    )?;
    fs::write(
        dir.path().join("artifacts"),
        format!(
            r#"[{{"total_count":1,"artifacts":[{{"id":53,"name":"image-assets-2","expired":false,"size_in_bytes":10,"workflow_run":{{"id":31,"head_sha":"{sha}","head_branch":"main"}}}}]}}]"#
        ),
    )?;
    let job = format!(
        r#"{{"id":71,"name":"qualify-images","status":"in_progress","runner_name":"runner-1","head_sha":"{sha}","run_id":41,"run_attempt":2}}"#
    );
    fs::write(
        dir.path().join("jobs"),
        format!(r#"[{{"total_count":1,"jobs":[{job}]}}]"#),
    )?;
    let output = dir.path().join("output");
    let path = format!("{}:{}", dir.path().display(), std::env::var("PATH")?);
    let run = || -> std::io::Result<std::process::Output> {
        Command::new("/bin/bash")
            .arg(root.join("scripts/resolve-runner-family-producer.sh"))
            .env("PATH", &path)
            .env("FIXTURE", dir.path())
            .env("GH_TOKEN", "fixture")
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_SHA", &sha)
            .env("GITHUB_RUN_ID", "41")
            .env("GITHUB_RUN_ATTEMPT", "2")
            .env("RUNNER_NAME", "runner-1")
            .env("GITHUB_OUTPUT", &output)
            .output()
    };
    let positive = run()?;
    assert!(
        positive.status.success(),
        "valid producer identity rejected: {}",
        String::from_utf8_lossy(&positive.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output)?,
        "producer_run_id=31\nproducer_run_attempt=2\nproducer_artifact_id=53\nqualifier_job_id=71\n"
    );
    assert_invalid_discovery(dir.path(), &output, &job, &run)?;
    Ok(())
}

#[cfg(unix)]
fn assert_invalid_discovery(
    dir: &std::path::Path,
    output: &std::path::Path,
    job: &str,
    run: &impl Fn() -> std::io::Result<std::process::Output>,
) -> Result<(), Box<dyn Error>> {
    use std::fs;
    fs::write(output, "")?;
    fs::write(
        dir.join("jobs"),
        format!(r#"[{{"total_count":2,"jobs":[{job},{job}]}}]"#),
    )?;
    let duplicate = run()?;
    assert!(
        !duplicate.status.success(),
        "ambiguous qualifier job accepted"
    );
    assert_eq!(fs::read_to_string(output)?, "");
    fs::write(
        dir.join("jobs"),
        format!(r#"[{{"total_count":1,"jobs":[{job}]}}]"#),
    )?;
    let artifact_path = dir.join("artifacts");
    let original_artifacts = fs::read(&artifact_path)?;
    let mut truncated: serde_json::Value = serde_json::from_slice(&original_artifacts)?;
    truncated[0]["total_count"] = serde_json::json!(2);
    fs::write(&artifact_path, serde_json::to_vec(&truncated)?)?;
    assert!(
        !run()?.status.success(),
        "truncated artifact inventory accepted"
    );
    assert_eq!(fs::read_to_string(output)?, "");
    fs::write(&artifact_path, original_artifacts)?;
    let mut capped: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("runs"))?)?;
    capped[0]["total_count"] = serde_json::json!(1000);
    let runs = capped[0]["workflow_runs"]
        .as_array_mut()
        .ok_or("runs missing")?;
    runs.extend((0..999).map(|id| serde_json::json!({"id": id + 100})));
    fs::write(dir.join("runs"), serde_json::to_vec(&capped)?)?;
    assert!(
        !run()?.status.success(),
        "truncated workflow inventory accepted"
    );
    assert_eq!(fs::read_to_string(output)?, "");
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    dead_code,
    reason = "shared public fixture supports additional admission cases"
)]

use std::{
    error::Error,
    fmt::Write as _,
    fs,
    path::Path,
    process::{Command, Output},
};

use serde_json::json;
use sha2::{Digest, Sha256};
use velnor_actions_contract::parse_runner_image_qualification;

#[path = "runner_image_admission_public/support/mod.rs"]
mod support;
use support::AdmissionFixture;

fn command(fixture: &AdmissionFixture, workflow: &str, run_id: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_REPOSITORY_ID", "1390620900")
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_SHA", &fixture.source)
        .env("GITHUB_WORKFLOW_SHA", &fixture.source)
        .env(
            "GITHUB_WORKFLOW_REF",
            format!("tailrocks/velnor-new/.github/workflows/{workflow}@refs/heads/main"),
        )
        .env("GITHUB_RUN_ID", run_id)
        .env("GITHUB_RUN_ATTEMPT", "2");
    command
}

fn produce(fixture: &AdmissionFixture, directory: &Path) -> Result<Output, std::io::Error> {
    command(fixture, "ci.yml", "111")
        .args(["image-maintenance", "record-runner-candidates", "--root"])
        .arg(&fixture.root)
        .args(["--source-commit", &fixture.source, "--runner-archive"])
        .arg(&fixture.images.runner.path)
        .arg("--dind-archive")
        .arg(&fixture.images.dind.path)
        .arg("--probe-archive")
        .arg(&fixture.images.resource_probe.path)
        .arg("--output-directory")
        .arg(directory)
        .output()
}

fn qualify(
    fixture: &AdmissionFixture,
    directory: &Path,
    runtime: &Path,
    output: &Path,
) -> Result<Output, std::io::Error> {
    qualify_with_producer(fixture, directory, runtime, output, "111")
}

fn qualify_with_producer(
    fixture: &AdmissionFixture,
    directory: &Path,
    runtime: &Path,
    output: &Path,
    producer: &str,
) -> Result<Output, std::io::Error> {
    command(fixture, "runner-image-qualification.yml", "222")
        .env("GITHUB_JOB", "qualify-images")
        .args(["image-maintenance", "write-runner-qualification", "--root"])
        .arg(&fixture.root)
        .args(["--source-commit", &fixture.source, "--runner-archive"])
        .arg(&fixture.images.runner.path)
        .arg("--runner-record")
        .arg(directory.join("runner-image-publication.json"))
        .arg("--dind-archive")
        .arg(&fixture.images.dind.path)
        .arg("--dind-record")
        .arg(directory.join("dind-image-publication.json"))
        .arg("--probe-archive")
        .arg(&fixture.images.resource_probe.path)
        .arg("--probe-record")
        .arg(directory.join("resource-probe-image-publication.json"))
        .arg("--runtime-result")
        .arg(runtime)
        .args([
            "--producer-run-id",
            producer,
            "--producer-run-attempt",
            "2",
            "--qualifier-job-id",
            "333",
        ])
        .arg("--output")
        .arg(output)
        .output()
}

fn lock_sha256(fixture: &AdmissionFixture) -> Result<String, Box<dyn Error>> {
    let lock = fs::read(fixture.root.join(".velnor/runner.lock"))?;
    let mut digest = String::new();
    for byte in Sha256::digest(lock) {
        write!(digest, "{byte:02x}")?;
    }
    Ok(digest)
}

fn reject_runtime_mismatches(
    fixture: &AdmissionFixture,
    directory: &Path,
    runtime: &Path,
    output: &Path,
    value: &serde_json::Value,
) -> Result<(), Box<dyn Error>> {
    for field in ["runner", "dind", "resource_probe"] {
        let mut invalid = value.clone();
        invalid["images"][field]["runtime_id"] = json!(format!("sha256:{}", "0".repeat(64)));
        fs::write(runtime, serde_json::to_vec(&invalid)?)?;
        assert!(
            !qualify(fixture, directory, runtime, output)?
                .status
                .success()
        );
        assert!(!output.exists());
    }
    for field in ["source_commit", "runner_lock_sha256"] {
        let mut invalid = value.clone();
        invalid[field] = json!("wrong");
        fs::write(runtime, serde_json::to_vec(&invalid)?)?;
        assert!(
            !qualify(fixture, directory, runtime, output)?
                .status
                .success()
        );
        assert!(!output.exists());
    }
    let mut extra = value.clone();
    extra["images"]["extra"] = json!("unexpected");
    fs::write(runtime, serde_json::to_vec(&extra)?)?;
    assert!(
        !qualify(fixture, directory, runtime, output)?
            .status
            .success()
    );
    let mut missing = value.clone();
    missing["images"]
        .as_object_mut()
        .ok_or("images not object")?
        .remove("dind");
    fs::write(runtime, serde_json::to_vec(&missing)?)?;
    assert!(
        !qualify(fixture, directory, runtime, output)?
            .status
            .success()
    );
    let duplicate = serde_json::to_string(value)?
        .replace("\"runner\":", "\"runner\":\"duplicate\",\"runner\":");
    fs::write(runtime, duplicate)?;
    assert!(
        !qualify(fixture, directory, runtime, output)?
            .status
            .success()
    );
    fs::write(runtime, serde_json::to_vec(value)?)?;
    assert!(
        !qualify_with_producer(fixture, directory, runtime, output, "222")?
            .status
            .success()
    );
    Ok(())
}

fn reject_record_mismatches(
    fixture: &AdmissionFixture,
    directory: &Path,
    runtime: &Path,
    output: &Path,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let record_path = directory.join("runner-image-publication.json");
    let record = fs::read(&record_path)?;
    let mut wrong_run: serde_json::Value = serde_json::from_slice(&record)?;
    wrong_run["workflow"]["run_attempt"] = json!(3);
    fs::write(&record_path, serde_json::to_vec(&wrong_run)?)?;
    assert!(
        !qualify(fixture, directory, runtime, output)?
            .status
            .success()
    );
    fs::write(&record_path, &record)?;
    let original_record = fs::read(&record_path)?;
    let mut wrong_source: serde_json::Value = serde_json::from_slice(&original_record)?;
    wrong_source["source"]["commit"] = json!("0".repeat(40));
    fs::write(&record_path, serde_json::to_vec(&wrong_source)?)?;
    assert!(
        !qualify(fixture, directory, runtime, output)?
            .status
            .success()
    );
    let mut wrong_lock: serde_json::Value = serde_json::from_slice(&original_record)?;
    wrong_lock["lock"]["sha256"] = json!("0".repeat(64));
    fs::write(&record_path, serde_json::to_vec(&wrong_lock)?)?;
    assert!(
        !qualify(fixture, directory, runtime, output)?
            .status
            .success()
    );
    fs::write(&record_path, &original_record)?;
    let archive = fs::read(&fixture.images.runner.path)?;
    fs::write(&fixture.images.runner.path, b"corrupt")?;
    assert!(
        !qualify(fixture, directory, runtime, output)?
            .status
            .success()
    );
    fs::write(&fixture.images.runner.path, archive)?;
    Ok(original_record)
}

fn assert_qualified_proof(
    fixture: &AdmissionFixture,
    directory: &Path,
    output: &Path,
    lock_sha: &str,
) -> Result<(), Box<dyn Error>> {
    let proof = parse_runner_image_qualification(&fs::read(output)?)?;
    assert_eq!(proof.source_commit, fixture.source);
    assert_eq!(proof.producer_run_id, 111);
    assert_eq!(proof.qualifier_run_id, 222);
    assert_eq!(proof.qualifier_run_attempt, 2);
    assert_eq!(proof.qualifier_job_id, 333);
    assert_eq!(proof.runner_lock_sha256, lock_sha);
    for (filename, image) in [
        ("runner-image-publication.json", &proof.images.runner),
        ("dind-image-publication.json", &proof.images.dind),
        (
            "resource-probe-image-publication.json",
            &proof.images.resource_probe,
        ),
    ] {
        let record = velnor_actions_contract::parse_runner_image_publication_record(&fs::read(
            directory.join(filename),
        )?)?;
        assert_eq!(image.archive, record.archive);
        assert_eq!(image.oci, record.oci);
    }
    Ok(())
}

#[test]
fn qualified_runtime_is_bound_to_exact_ci_archives() -> Result<(), Box<dyn Error>> {
    let fixture = AdmissionFixture::full()?;
    let directory = fixture.image_dir.join("records");
    fs::create_dir(&directory)?;
    let produced = produce(&fixture, &directory)?;
    assert!(
        produced.status.success(),
        "{}",
        String::from_utf8_lossy(&produced.stderr)
    );
    let runtime = fixture.image_dir.join("runtime.json");
    let output = fixture.image_dir.join("RUNNER_IMAGE_QUALIFICATION.json");
    let lock_sha = lock_sha256(&fixture)?;
    let value = json!({"schema": "velnor.runner-image-runtime-qualification.v1", "source_commit": fixture.source,
        "runner_lock_sha256": lock_sha, "images": {"runner": {"runtime_id": fixture.images.runner.config_digest, "identity_kind": "config"},
        "dind": {"runtime_id": fixture.images.dind.config_digest, "identity_kind": "config"}, "resource_probe": {"runtime_id": fixture.images.resource_probe.config_digest, "identity_kind": "config"}}});
    reject_runtime_mismatches(&fixture, &directory, &runtime, &output, &value)?;
    reject_record_mismatches(&fixture, &directory, &runtime, &output)?;
    let result = qualify(&fixture, &directory, &runtime, &output)?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_qualified_proof(&fixture, &directory, &output, &lock_sha)?;
    let manifest_output = fixture.image_dir.join("manifest-qualification.json");
    let mut manifest_runtime = value.clone();
    for (role, digest) in [
        ("runner", &fixture.images.runner.manifest_digest),
        ("dind", &fixture.images.dind.manifest_digest),
        (
            "resource_probe",
            &fixture.images.resource_probe.manifest_digest,
        ),
    ] {
        manifest_runtime["images"][role] =
            json!({"runtime_id": digest, "identity_kind": "platform_manifest"});
    }
    fs::write(&runtime, serde_json::to_vec(&manifest_runtime)?)?;
    let result = qualify(&fixture, &directory, &runtime, &manifest_output)?;
    assert!(
        result.status.success(),
        "manifest runtime IDs: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    for role in ["runner", "dind", "resource_probe"] {
        let mut wrong_kind = manifest_runtime.clone();
        wrong_kind["images"][role]["identity_kind"] = json!("config");
        fs::write(&runtime, serde_json::to_vec(&wrong_kind)?)?;
        let rejected = fixture.image_dir.join(format!("wrong-{role}.json"));
        assert!(
            !qualify(&fixture, &directory, &runtime, &rejected)?
                .status
                .success()
        );
        assert!(!rejected.exists());
    }
    fs::write(&runtime, serde_json::to_vec(&value)?)?;
    let original = fs::read(&output)?;
    assert!(
        !qualify(&fixture, &directory, &runtime, &output)?
            .status
            .success()
    );
    assert_eq!(fs::read(output)?, original);
    Ok(())
}

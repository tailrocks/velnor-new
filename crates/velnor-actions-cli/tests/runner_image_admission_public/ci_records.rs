// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn ci_records_exact_runner_candidates_and_rejects_stale_outputs() -> Result<(), Box<dyn Error>> {
    use std::process::Command;
    use velnor_actions_contract::parse_runner_image_publication_record;

    let fixture = AdmissionFixture::full()?;
    let output_dir = fixture.image_dir.join("records");
    fs::create_dir(&output_dir)?;
    let invoke = |workflow: &str, source: &str| -> Result<std::process::Output, std::io::Error> {
        Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_REPOSITORY_ID", "1390620900")
            .env("GITHUB_EVENT_NAME", "push")
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_SHA", source)
            .env("GITHUB_WORKFLOW_SHA", source)
            .env("GITHUB_WORKFLOW_REF", workflow)
            .env("GITHUB_RUN_ID", "111")
            .env("GITHUB_RUN_ATTEMPT", "2")
            .args(["image-maintenance", "record-runner-candidates", "--root"])
            .arg(&fixture.root)
            .args(["--source-commit", source, "--runner-archive"])
            .arg(&fixture.images.runner.path)
            .arg("--dind-archive")
            .arg(&fixture.images.dind.path)
            .arg("--probe-archive")
            .arg(&fixture.images.resource_probe.path)
            .arg("--output-directory")
            .arg(&output_dir)
            .output()
    };
    let bad = invoke(
        "tailrocks/velnor-new/.github/workflows/other.yml@refs/heads/main",
        &fixture.source,
    )?;
    assert!(!bad.status.success());
    assert_eq!(fs::read_dir(&output_dir)?.count(), 0);
    let wrong_source = "0".repeat(40);
    assert!(
        !invoke(
            "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
            &wrong_source
        )?
        .status
        .success()
    );
    assert_eq!(fs::read_dir(&output_dir)?.count(), 0);
    let original_archive = fs::read(&fixture.images.dind.path)?;
    fs::write(&fixture.images.dind.path, b"invalid OCI archive")?;
    let corrupt = invoke(
        "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
        &fixture.source,
    )?;
    assert!(!corrupt.status.success());
    assert_eq!(fs::read_dir(&output_dir)?.count(), 0);
    fs::write(&fixture.images.dind.path, original_archive)?;
    let lock_path = fixture.root.join(".velnor/runner.lock");
    let lock = fs::read(&lock_path)?;
    fs::write(&lock_path, b"runner_version = \"0.0.0\"\n")?;
    assert!(
        !invoke(
            "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
            &fixture.source
        )?
        .status
        .success()
    );
    assert_eq!(fs::read_dir(&output_dir)?.count(), 0);
    fs::write(&lock_path, &lock)?;
    let good = invoke(
        "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
        &fixture.source,
    )?;
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stderr)
    );
    assert_eq!(fs::read_dir(&output_dir)?.count(), 3);
    for (filename, role, image) in [
        (
            "runner-image-publication.json",
            "runner",
            &fixture.images.runner,
        ),
        ("dind-image-publication.json", "dind", &fixture.images.dind),
        (
            "resource-probe-image-publication.json",
            "resource_probe",
            &fixture.images.resource_probe,
        ),
    ] {
        let record = parse_runner_image_publication_record(&fs::read(output_dir.join(filename))?)?;
        assert_eq!(record.image, role);
        assert_eq!(record.source.commit, fixture.source);
        assert_eq!(record.workflow.run_id, 111);
        assert_eq!(record.workflow.run_attempt, 2);
        assert_eq!(
            record.workflow.workflow_ref,
            "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main"
        );
        assert_eq!(record.lock.sha256, lock_sha256(&lock));
        assert_eq!(record.archive.sha256, lock_sha256(&fs::read(&image.path)?));
        assert_eq!(record.oci.platform_manifest_digest, image.manifest_digest);
        assert_eq!(record.oci.image_configuration_digest, image.config_digest);
    }
    let existing = fs::read(output_dir.join("runner-image-publication.json"))?;
    let repeated = invoke(
        "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main",
        &fixture.source,
    )?;
    assert!(!repeated.status.success());
    assert_eq!(
        fs::read(output_dir.join("runner-image-publication.json"))?,
        existing
    );
    Ok(())
}

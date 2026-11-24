// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, ImageError, expected_files};

use crate::facts::TestResult;
use crate::image_workflow::{SAMPLES, edited, image, jobs, name_of, rendered};

const POOL: &str = "ubuntu-26.04-scale-set";
const PUBLISH_POOL: &str = "ubuntu-26.04-scale-set-publish";

#[test]
fn the_polling_gate_and_the_authority_job_never_hold_a_payload_runner() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            if id == "image-gate" || id.ends_with("-authority") {
                assert_ne!(job["runs-on"], POOL, "{name} {id}");
                assert_ne!(job["runs-on"], PUBLISH_POOL, "{name} {id}");
            }
        }
    }
    let plain = rendered("image-plain")?;
    assert_eq!(plain["jobs"]["image-gate"]["runs-on"], "ubuntu-latest");
    assert_eq!(
        plain["jobs"]["image-plain-svc-authority"]["runs-on"],
        "ubuntu-latest"
    );
    let ghcr = rendered("image-ghcr")?;
    assert_eq!(ghcr["jobs"]["image-gate"]["runs-on"], "ubuntu-24.04");
    assert_eq!(
        ghcr["jobs"]["image-cluster-svc-authority"]["runs-on"],
        "ubuntu-latest"
    );
    Ok(())
}

#[test]
fn a_configured_control_runner_hosts_the_gate_and_the_authority_job() -> TestResult {
    let view = edited("image-plain", |text| {
        text.replace(
            "attestations = false\n",
            "attestations = false\ncontrol_runner = \"ubuntu-24.04\"\n",
        )
    })?;
    let workflow = image(&view)?;
    assert_eq!(workflow["jobs"]["image-gate"]["runs-on"], "ubuntu-24.04");
    assert_eq!(
        workflow["jobs"]["image-plain-svc-authority"]["runs-on"],
        "ubuntu-24.04"
    );
    Ok(())
}

#[test]
fn registry_jobs_run_on_the_publish_pool_and_pull_request_code_on_the_payload_pool() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            let registry = id.ends_with("-publish") || id.ends_with("-promote");
            let payload = id.ends_with("-build") || id.ends_with("-check");
            if registry {
                assert_eq!(job["runs-on"], PUBLISH_POOL, "{name} {id}");
            }
            if payload {
                assert_eq!(job["runs-on"], POOL, "{name} {id}");
            }
        }
    }
    Ok(())
}

#[test]
fn a_self_hosted_pool_must_name_its_publish_pool_while_a_hosted_runner_needs_none() -> TestResult {
    let shared = edited("image-plain", |text| {
        text.replace("publish_runner = \"ubuntu-26.04-scale-set-publish\"\n", "")
    })?;
    assert!(matches!(
        expected_files(&shared),
        Err(GenerateError::Image(ImageError::PublishPoolShared { .. }))
    ));
    let acknowledged = edited("image-plain", |text| {
        text.replace(
            "publish_runner = \"ubuntu-26.04-scale-set-publish\"",
            "publish_runner = \"ubuntu-26.04-scale-set\"",
        )
    })?;
    let workflow = image(&acknowledged)?;
    assert_eq!(workflow["jobs"]["image-plain-svc-publish"]["runs-on"], POOL);
    let hosted = edited("image-plain", |text| {
        text.replace(
            "runner = \"ubuntu-26.04-scale-set\"\nhosted = false",
            "runner = \"ubuntu-24.04\"\nhosted = true",
        )
        .replace("publish_runner = \"ubuntu-26.04-scale-set-publish\"\n", "")
    })?;
    let workflow = image(&hosted)?;
    for part in ["build", "publish"] {
        assert_eq!(
            workflow["jobs"][format!("image-{}-{part}", name_of("image-plain"))]["runs-on"],
            "ubuntu-24.04"
        );
    }
    Ok(())
}

#[test]
fn images_cannot_disagree_on_the_one_gate_and_every_runner_must_be_linux_x64() -> TestResult {
    let second = "\n[[image]]\nname = \"other-svc\"\nbins = [\"plain-svc\"]\nstage = \"dist-other\"\ncontrol_runner = \"ubuntu-24.04\"\npublish_runner = \"ubuntu-26.04-scale-set-publish\"\n\n[[image.registry]]\nhost = \"ghcr.io\"\nrepository = \"acme/other-svc\"\n";
    let view = edited("image-plain", |text| format!("{text}{second}"))?;
    assert!(matches!(
        expected_files(&view),
        Err(GenerateError::Image(
            ImageError::ControlRunnerDiffers { .. }
        ))
    ));
    for key in ["control_runner", "publish_runner"] {
        let view = edited("image-plain", |text| {
            text.replace(
                "publish_runner = \"ubuntu-26.04-scale-set-publish\"\n",
                &format!("{key} = \"macos-15\"\n"),
            )
        })?;
        assert!(
            matches!(
                expected_files(&view),
                Err(GenerateError::Image(ImageError::NotLinuxRunner { .. }))
            ),
            "{key}: {:?}",
            expected_files(&view).err()
        );
    }
    Ok(())
}

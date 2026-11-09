// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn release_tool_expectations_require_a_valid_consumer_manifest()
-> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest as _, Sha256};
    use std::fmt::Write as _;

    let version = env!("CARGO_PKG_VERSION");
    let host = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        _ => return Err("unsupported test host".into()),
    };
    let executable = std::fs::read(std::env::current_exe()?)?;
    let mut digest = String::new();
    for byte in Sha256::digest(executable) {
        write!(digest, "{byte:02x}")?;
    }
    let raw = consumer_manifest(version, host, &digest);
    let acquire = |json: &str| {
        crate::planner::pins::consumer_acquire_step_with_manifest(
            "ubuntu-26.04",
            version,
            Some(json),
        )
    };
    assert!(acquire(&raw).is_ok());
    let mut wrong_binary: serde_json::Value = serde_json::from_str(&raw)?;
    let targets = wrong_binary["targets"].as_array_mut().ok_or("targets")?;
    let host_record = targets
        .iter_mut()
        .find(|target| target["target"] == host)
        .ok_or("host target")?;
    host_record["sha256"] = if digest == "b".repeat(64) {
        "c".repeat(64)
    } else {
        "b".repeat(64)
    }
    .into();
    let Err(error) = acquire(&serde_json::to_string(&wrong_binary)?) else {
        return Err("wrong host digest must fail".into());
    };
    assert!(
        error
            .to_string()
            .contains("release_manifest_binary_mismatch")
    );
    let mut other_binary: serde_json::Value = serde_json::from_str(&raw)?;
    let targets = other_binary["targets"].as_array_mut().ok_or("targets")?;
    let other_record = targets
        .iter_mut()
        .find(|target| target["target"] != host)
        .ok_or("other target")?;
    other_record["sha256"] = "b".repeat(64).into();
    assert!(acquire(&serde_json::to_string(&other_binary)?).is_ok());
    let mut wrong: serde_json::Value = serde_json::from_str(&raw)?;
    wrong["version"] = "9.9.9".into();
    assert!(acquire(&serde_json::to_string(&wrong)?).is_err());
    let mut missing: serde_json::Value = serde_json::from_str(&raw)?;
    missing["targets"]
        .as_array_mut()
        .ok_or("targets")?
        .retain(|target| target["target"] != "x86_64-unknown-linux-gnu");
    assert!(acquire(&serde_json::to_string(&missing)?).is_err());
    let mut duplicate: serde_json::Value = serde_json::from_str(&raw)?;
    let targets = duplicate["targets"].as_array_mut().ok_or("targets")?;
    targets.push(targets[0].clone());
    assert!(acquire(&serde_json::to_string(&duplicate)?).is_err());
    let mut malformed: serde_json::Value = serde_json::from_str(&raw)?;
    malformed["targets"][0]["sha256"] = "not-a-digest".into();
    assert!(acquire(&serde_json::to_string(&malformed)?).is_err());
    Ok(())
}

#[test]
fn publishing_jobs_verify_generator_before_running_it() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_contract::StepKind;
    use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_workflow,
    };
    let context = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    for version in ["1..2", "1.2.3-rc.1", "latest"] {
        assert!(
            velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(
                version,
                "ubuntu-26.04",
                &"b".repeat(40),
                &"c".repeat(64)
            )
            .is_err()
        );
    }
    assert!(
        velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(
            "1.2.3",
            "ubuntu-24.04",
            &"b".repeat(40),
            &"c".repeat(64)
        )
        .is_err()
    );
    for authentication in [
        ReleaseAuthentication::TrustedPublishing,
        ReleaseAuthentication::BootstrapToken,
    ] {
        let (spec, rust) = fixture_for(authentication)?;
        render_release_workflow(&spec, &context, &rust)?;
        let publisher = spec
            .jobs
            .values()
            .find(|job| {
                matches!(
                    job.role,
                    ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap
                )
            })
            .ok_or("publisher")?;
        assert_eq!(publisher.steps.len(), 13);
        assert_eq!(publisher.steps[0].name, "Acquire Velnor");
        let mut tampered = spec.clone();
        let publisher = tampered
            .jobs
            .values_mut()
            .find(|job| {
                matches!(
                    job.role,
                    ReleaseRole::PublishOidc | ReleaseRole::PublishBootstrap
                )
            })
            .ok_or("publisher")?;
        let StepKind::Shell { run, .. } = &mut publisher.steps[0].kind else {
            return Err("publisher shell".into());
        };
        let last = run.len() - 1;
        run[last] = "d".repeat(64);
        assert!(render_release_workflow(&tampered, &context, &rust).is_err());
        let preflight = spec.jobs.get("release-preflight").ok_or("preflight")?;
        let acquire = preflight
            .steps
            .iter()
            .find(|step| step.name == "Acquire Velnor")
            .ok_or("acquire")?;
        let StepKind::Shell { env, .. } = &acquire.kind else {
            return Err("acquire shell".into());
        };
        assert_eq!(
            env.get("VELNOR_GENERATOR_VERSION").map(String::as_str),
            Some(env!("CARGO_PKG_VERSION"))
        );
    }
    Ok(())
}

#[test]
fn rendered_bootstrap_rejects_untrusted_generator_before_staging()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use velnor_actions_workflow_renderer::release_tree::{
        ReleaseRenderContext, render_release_workflow,
    };
    let (spec, rust) = fixture()?;
    let context = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        runs_on: "ubuntu-26.04".into(),
    };
    let workflow = render_release_workflow(&spec, &context, &rust)?;
    let temp = tempfile::tempdir()?;
    let harness = temp.path().join("bootstrap_harness.py");
    std::fs::write(
        &harness,
        include_str!("release_steps_tests/bootstrap_harness.py"),
    )?;
    let mut child = Command::new("python3")
        .arg(harness)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("missing stdin")?
        .write_all(workflow.as_bytes())?;
    let output = child.wait_with_output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

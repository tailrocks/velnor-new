// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::{ReleaseRehearsalConfig, RustReleaseConfig};
use velnor_actions_workflow_renderer::release_tree::rehearsal;

#[cfg(unix)]
#[test]
fn metadata_ignores_source_cargo_configuration() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = std::process::Command::new("python3")
        .arg(root.join("tests/release_rehearsal_metadata_fixture.py"))
        .arg(root.join("src/release_rehearsal_metadata.py"))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn metadata_is_opt_in_and_bound_to_an_isolated_reviewed_source_job()
-> Result<(), Box<dyn std::error::Error>> {
    let baseline = rehearsal::render(&config(), "0.1.73")?;
    let baseline: serde_json::Value = serde_saphyr::from_str(&baseline.bytes)?;
    assert!(baseline["jobs"].get("metadata").is_none());
    let mut rehearsal = config();
    rehearsal.metadata = Some(
        velnor_actions_contract::config::ReleaseRehearsalMetadataConfig {
            manifest_path: "Cargo.toml".into(),
            intent_config_path: ".velnor/config.toml".into(),
            qualification: policy(),
            rust_version: "1.97.1".into(),
        },
    );
    let output = rehearsal::render(&rehearsal, "0.1.73")?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&output.bytes)?;
    assert_eq!(
        workflow["on"]["workflow_dispatch"]["inputs"]["source_sha"]["required"],
        true
    );
    assert_eq!(
        workflow["jobs"]["metadata"]["permissions"],
        serde_json::json!({})
    );
    assert_eq!(workflow["jobs"]["metadata"]["needs"], "gate");
    assert_eq!(
        workflow["jobs"]["witness"]["needs"],
        serde_json::json!(["gate", "execute", "metadata"])
    );
    let steps = workflow["jobs"]["metadata"]["steps"]
        .as_array()
        .ok_or("metadata steps")?;
    assert_eq!(
        steps
            .iter()
            .filter(|s| s["name"] == velnor_actions_workflow_renderer::VERIFY_RUST_TOOLCHAIN_STEP)
            .count(),
        2
    );
    let script = steps
        .iter()
        .find(|s| s["name"] == "Compute approved source metadata")
        .and_then(|s| s["run"].as_str())
        .ok_or("metadata script")?;
    assert!(script.contains("--cap-drop=ALL"));
    assert!(script.contains("dst=/source,readonly"));
    assert!(script.contains("--locked"));
    assert!(
        workflow["jobs"]["witness"]["steps"]
            .as_array()
            .ok_or("witness steps")?
            .iter()
            .any(|s| s["name"] == "Bind approved metadata source")
    );
    let binding = workflow["jobs"]["witness"]["steps"]
        .as_array()
        .ok_or("witness steps")?
        .iter()
        .find(|s| s["name"] == "Bind approved metadata source")
        .ok_or("binding")?;
    assert_eq!(binding["env"]["INTENT_CONFIG_PATH"], ".velnor/config.toml");
    rehearsal
        .metadata
        .as_mut()
        .ok_or("metadata")?
        .intent_config_path = "../secret.toml".into();
    assert!(rehearsal::render(&rehearsal, "0.1.73").is_err());
    let metadata = rehearsal.metadata.as_mut().ok_or("metadata")?;
    metadata.intent_config_path = ".velnor/config.toml".into();
    let velnor_actions_contract::config::ReleaseReviewProof::CompletionCheck {
        completion_check_name,
        ..
    } = &mut metadata.qualification.review_proof
    else {
        return Err("expected completion-check policy".into());
    };
    *completion_check_name = "${{ secrets.TOKEN }}".into();
    assert!(rehearsal::render(&rehearsal, "0.1.73").is_err());
    Ok(())
}

fn policy() -> velnor_actions_contract::config::ReleaseQualificationPolicy {
    velnor_actions_contract::config::ReleaseQualificationPolicy {
        candidate_repository: "candidate/repo".into(),
        candidate_repository_id: 1,
        candidate_default_branch: "main".into(),
        candidate_required_workflow_path: ".github/workflows/ci.yml".into(),
        review_proof: velnor_actions_contract::config::ReleaseReviewProof::CompletionCheck {
            completion_app_id: 1,
            completion_check_name: "complete".into(),
            reviewer_app_ids: [1].into_iter().collect(),
        },
        historical_manifest: velnor_actions_contract::config::ReleaseHistoricalManifestPolicy {
            version: "0.1.5".into(),
            source_sha: "b".repeat(40),
            sha256: "c".repeat(64),
        },
    }
}

fn config() -> ReleaseRehearsalConfig {
    ReleaseRehearsalConfig {
        script: "qualification/rehearse.sh".into(),
        environment: "qualification".into(),
        reviewer_id: 42,
        output_bundle: false,
        metadata: None,
    }
}

#[test]
fn runner_can_be_enabled_independently_of_publication() -> Result<(), Box<dyn std::error::Error>> {
    let release = RustReleaseConfig {
        rehearsal: Some(config()),
        ..RustReleaseConfig::default()
    };
    release.validate("config.toml")?;
    let file = rehearsal::render(
        release.rehearsal.as_ref().ok_or("missing config")?,
        "0.1.73",
    )?;
    assert_eq!(file.path, ".github/workflows/release-rehearsal.yml");
    let workflow: serde_json::Value = serde_saphyr::from_str(&file.bytes)?;
    assert!(workflow["on"]["workflow_dispatch"].is_object());
    assert!(workflow["on"].get("push").is_none());
    assert_eq!(workflow["jobs"]["gate"]["environment"], "qualification");
    assert_eq!(
        workflow["jobs"]["execute"]["permissions"],
        serde_json::json!({})
    );
    assert_eq!(workflow["jobs"]["execute"]["needs"], "gate");
    assert_eq!(
        workflow["jobs"]["witness"]["permissions"],
        serde_json::json!({})
    );
    assert_eq!(
        workflow["jobs"]["witness"]["needs"],
        serde_json::json!(["gate", "execute"])
    );
    let witness = workflow["jobs"]["witness"]["steps"][0]["run"]
        .as_str()
        .ok_or("missing witness")?;
    assert!(witness.contains("script-exited-successfully"));
    assert!(witness.contains("unverified_candidate_sha"));
    Ok(())
}

#[test]
fn output_bundle_uses_separate_untrusted_and_witness_artifacts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut rehearsal = config();
    rehearsal.output_bundle = true;
    let rendered = rehearsal::render(&rehearsal, "0.1.73")?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&rendered.bytes)?;
    let execute = workflow["jobs"]["execute"]["steps"]
        .as_array()
        .ok_or("missing execute")?;
    let witness = workflow["jobs"]["witness"]["steps"]
        .as_array()
        .ok_or("missing witness")?;
    assert!(
        execute
            .iter()
            .any(|s| s["with"]["name"] == "rehearsal-output-untrusted")
    );
    assert!(
        witness
            .iter()
            .any(|s| s["with"]["name"] == "rehearsal-output-untrusted")
    );
    assert!(
        witness
            .iter()
            .any(|s| s["with"]["name"] == "rehearsal-output-observation")
    );
    assert!(
        witness
            .iter()
            .any(|s| s["name"] == "Bind rehearsal output artifact")
    );
    assert!(
        execute
            .iter()
            .filter_map(|s| s["run"].as_str())
            .any(|s| s.contains("VELNOR_REHEARSAL_OUTPUT=$RUNNER_TEMP/rehearsal-output.tar"))
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn output_validation_rejects_symlink_and_oversized_file() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::symlink;
    let mut rehearsal = config();
    rehearsal.output_bundle = true;
    let rendered = rehearsal::render(&rehearsal, "0.1.73")?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&rendered.bytes)?;
    for (job, name, relative) in [
        (
            "execute",
            "Validate untrusted rehearsal output",
            "rehearsal-output.tar",
        ),
        (
            "witness",
            "Validate transferred rehearsal output",
            "output/rehearsal-output.tar",
        ),
    ] {
        let steps = workflow["jobs"][job]["steps"]
            .as_array()
            .ok_or("missing steps")?;
        let script = steps
            .iter()
            .find(|step| step["name"] == name)
            .and_then(|step| step["run"].as_str())
            .ok_or("missing validator")?;
        let temp = tempfile::tempdir()?;
        let output = temp.path().join(relative);
        std::fs::create_dir_all(output.parent().ok_or("output parent")?)?;
        let run = || {
            std::process::Command::new("bash")
                .arg("-c")
                .arg(script)
                .env("RUNNER_TEMP", temp.path())
                .status()
        };
        assert!(!run()?.success(), "{job}: missing");
        std::fs::write(&output, b"")?;
        assert!(!run()?.success(), "{job}: empty");
        std::fs::write(&output, b"bundle")?;
        assert!(run()?.success());
        std::fs::write(&output, b"x")?;
        assert!(run()?.success(), "{job}: one byte");
        std::fs::remove_file(&output)?;
        symlink("elsewhere", &output)?;
        assert!(!run()?.success());
        std::fs::remove_file(&output)?;
        let target = temp.path().join("nonempty");
        std::fs::write(&target, b"bundle")?;
        symlink(&target, &output)?;
        assert!(!run()?.success(), "{job}: regular-file symlink");
        std::fs::remove_file(&output)?;
        std::fs::File::create(&output)?.set_len(100 * 1024 * 1024)?;
        assert!(run()?.success(), "{job}: exact limit");
        std::fs::File::create(&output)?.set_len(100 * 1024 * 1024 + 1)?;
        assert!(!run()?.success());
        std::fs::remove_file(&output)?;
        std::fs::create_dir(&output)?;
        assert!(!run()?.success(), "{job}: directory");
    }
    Ok(())
}

#[test]
fn rehearsal_rejects_unsafe_script_paths_and_unbound_reviewer() {
    for script in [
        "",
        "../outside.sh",
        "/absolute.sh",
        "dir//file.sh",
        "$(echo x).sh",
    ] {
        let mut rehearsal = config();
        rehearsal.script = script.into();
        let release = RustReleaseConfig {
            rehearsal: Some(rehearsal),
            ..RustReleaseConfig::default()
        };
        assert!(release.validate("config.toml").is_err(), "{script}");
    }
    let mut rehearsal = config();
    rehearsal.reviewer_id = 0;
    let release = RustReleaseConfig {
        rehearsal: Some(rehearsal),
        ..RustReleaseConfig::default()
    };
    assert!(release.validate("config.toml").is_err());
}

#[test]
fn rehearsal_scripts_enforce_their_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    let file = rehearsal::render(&config(), "0.1.73")?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&file.bytes)?;
    let dir = tempfile::tempdir()?;
    let scripts = [
        (
            "VELNOR_RENDERED_POLICY",
            &workflow["jobs"]["preflight"]["steps"][0]["run"],
        ),
        (
            "VELNOR_RENDERED_PREPARE",
            &workflow["jobs"]["gate"]["steps"][2]["run"],
        ),
        (
            "VELNOR_RENDERED_EXECUTE",
            &workflow["jobs"]["execute"]["steps"][1]["run"],
        ),
        (
            "VELNOR_RENDERED_WITNESS",
            &workflow["jobs"]["witness"]["steps"][0]["run"],
        ),
        (
            "VELNOR_RENDERED_MARKER",
            &workflow["jobs"]["witness"]["steps"][2]["run"],
        ),
    ];
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/release_rehearsal_behavior.py");
    let mut command = std::process::Command::new("python3");
    command.arg("-B").arg(script);
    for (name, value) in scripts {
        let path = dir.path().join(name);
        std::fs::write(&path, value.as_str().ok_or("rendered script missing")?)?;
        command.env(name, path);
    }
    let status = command.status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn metadata_rejects_rust_versions_without_a_verified_channel_manifest() {
    let mut rehearsal = config();
    rehearsal.metadata = Some(
        velnor_actions_contract::config::ReleaseRehearsalMetadataConfig {
            manifest_path: "Cargo.toml".into(),
            intent_config_path: ".velnor/config.toml".into(),
            qualification: policy(),
            rust_version: "1.0.0".into(),
        },
    );
    assert!(rehearsal.validate("test").is_err());
}

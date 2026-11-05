// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::config::{ReleaseRehearsalConfig, RustReleaseConfig};
use velnor_actions_workflow_renderer::release_tree::rehearsal;

fn config() -> ReleaseRehearsalConfig {
    ReleaseRehearsalConfig {
        script: "qualification/rehearse.sh".into(),
        environment: "qualification".into(),
        reviewer_id: 42,
        output_bundle: false,
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

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(target_os = "linux")]
use base64::Engine as _;
#[cfg(target_os = "linux")]
use std::io::Read as _;

#[cfg(target_os = "linux")]
#[test]
fn exported_privileged_closure_rejects_changed_modules_and_outer_entry()
-> Result<(), Box<dyn std::error::Error>> {
    let step = velnor_actions_workflow_renderer::release_root_publisher::publisher_step(
        false,
        env!("CARGO_PKG_VERSION"),
        &"b".repeat(40),
        &"c".repeat(64),
    )?;
    let velnor_actions_contract::StepKind::Shell { env, .. } = step.kind else {
        return Err("publisher bootstrap shell unavailable".into());
    };
    let encoded = env
        .get("VELNOR_RELEASE_BOOTSTRAP_PAYLOAD")
        .ok_or("bootstrap payload missing")?;
    let compressed = base64::engine::general_purpose::STANDARD.decode(encoded)?;
    let mut payload = Vec::new();
    lzma_rust2::XzReader::new(compressed.as_slice(), false)
        .take(512 * 1024 + 1)
        .read_to_end(&mut payload)?;
    assert!(payload.len() <= 512 * 1024);
    let temp = tempfile::tempdir()?;
    let bundle = temp.path().join("bundle.json");
    let entry = temp.path().join("entry.py");
    std::fs::write(&bundle, payload)?;
    std::fs::write(
        &entry,
        velnor_actions_workflow_renderer::release_root_publisher::render_entry(1800)?,
    )?;
    let status = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-B"])
        .arg(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/release_export_contract_test.py"),
        )
        .arg(bundle)
        .arg(entry)
        .env_clear()
        .status()?;
    assert!(
        status.success(),
        "full privileged export closure contract failed"
    );
    Ok(())
}

pub(super) fn assert_bootstrap_sources(bundle: &serde_json::Value) {
    for (key, expected) in [
        (
            "stage",
            include_str!("../src/release_publish_first_stage.py"),
        ),
        ("entry", include_str!("../src/release_publish_entry.py")),
        (
            "bootstrap",
            include_str!("../src/release_generator_bootstrap.sh"),
        ),
        ("manager", include_str!("../src/release_manager_bus.py")),
        (
            "watch_admission",
            include_str!("../src/release_watch_admission.py"),
        ),
        (
            "lifecycle",
            include_str!("../src/release_manager_lifecycle.py"),
        ),
        (
            "manager_policy_host",
            include_str!("../src/release_manager_policy_host.py"),
        ),
        (
            "manager_policy_files",
            include_str!("../src/release_manager_policy_files.py"),
        ),
        (
            "manager_policy",
            include_str!("../src/release_manager_policy.py"),
        ),
        ("helper", include_str!("../src/release_supervisor_exec.py")),
    ] {
        assert_eq!(bundle[key].as_str(), Some(expected));
    }
}

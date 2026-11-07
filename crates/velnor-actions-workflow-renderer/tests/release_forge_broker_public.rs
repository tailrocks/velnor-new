// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(target_os = "linux")]
#[test]
fn github_transport_fetches_complete_pull_request_lists() -> Result<(), Box<dyn std::error::Error>> {
    let status = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-B"])
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/release_forge_broker_test.py"))
        .env_clear()
        .status()?;
    assert!(status.success(), "forge broker transport contract failed");
    Ok(())
}

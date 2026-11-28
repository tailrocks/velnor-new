// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::{Command, Output};

fn disconnect(state: &std::path::Path) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .arg("--state")
        .arg(state)
        .args(["disconnect", "--connection-id", "lab"])
        .env_remove("HOME")
        .output()
}

fn doctor_probe(state: &std::path::Path) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .arg("--state")
        .arg(state)
        .args(["doctor", "--probe"])
        .env_remove("HOME")
        .env("VELNOR_ORB", "/nonexistent/orb")
        .output()
}

#[test]
fn an_absent_host_toml_keeps_the_official_mode_and_an_unreadable_one_fails_closed()
-> Result<(), Box<dyn Error>> {
    let state = tempfile::tempdir()?;
    let absent = doctor_probe(state.path())?;
    let absent_stderr = String::from_utf8_lossy(&absent.stderr);
    assert!(
        !absent_stderr.contains("(rust_runner)"),
        "a state directory without host.toml is not in rust-runner mode: {absent_stderr}"
    );
    std::fs::write(state.path().join("host.toml"), "schema = 2\n[[connections")?;
    let malformed = disconnect(state.path())?;
    assert_eq!(malformed.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&malformed.stderr);
    assert!(
        stderr.contains("Config cannot be read"),
        "a malformed host.toml must fail closed: {stderr}"
    );
    Ok(())
}

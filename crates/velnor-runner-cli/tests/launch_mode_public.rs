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

#[test]
fn a_host_toml_that_cannot_be_read_fails_the_command_before_any_mode_is_chosen()
-> Result<(), Box<dyn Error>> {
    let state = tempfile::tempdir()?;
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

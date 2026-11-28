// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::process::{Command, Output};

const UNAVAILABLE: &str = "this command is not available in rust-runner launch mode yet";

fn disconnect(state: &std::path::Path) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_velnor-host"))
        .arg("--state")
        .arg(state)
        .args(["disconnect", "--connection-id", "lab"])
        .env_remove("HOME")
        .output()
}

#[test]
fn a_host_toml_that_cannot_be_read_keeps_rust_runner_only_commands_unavailable()
-> Result<(), Box<dyn Error>> {
    let state = tempfile::tempdir()?;
    let absent = disconnect(state.path())?;
    assert!(
        !String::from_utf8_lossy(&absent.stderr).contains(UNAVAILABLE),
        "a state directory without host.toml is not in rust-runner mode: {}",
        String::from_utf8_lossy(&absent.stderr)
    );
    std::fs::write(state.path().join("host.toml"), "schema = 2\n[[connections")?;
    let malformed = disconnect(state.path())?;
    assert_eq!(
        malformed.status.code(),
        Some(1),
        "a malformed host.toml must keep rust-runner-only commands unavailable"
    );
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains(UNAVAILABLE),
        "a malformed host.toml must keep rust-runner-only commands unavailable: {}",
        String::from_utf8_lossy(&malformed.stderr)
    );
    Ok(())
}

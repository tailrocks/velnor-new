// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::in_rust_runner_mode;

#[test]
fn a_host_toml_that_cannot_be_read_counts_as_rust_runner_mode()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    assert!(
        !in_rust_runner_mode(temp.path()),
        "a state directory without host.toml is not in rust-runner mode"
    );
    std::fs::write(temp.path().join("host.toml"), "schema = 2\n[[connections")?;
    assert!(
        in_rust_runner_mode(temp.path()),
        "a malformed host.toml must keep rust-runner-only commands unavailable"
    );
    Ok(())
}

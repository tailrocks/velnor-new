// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type TestResult = Result<(), Box<dyn Error>>;

const BIN: &str = env!("CARGO_BIN_EXE_velnor-freshness");

fn root(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("freshness-bin-{name}"));
    match fs::remove_dir_all(&dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    fs::create_dir_all(dir.join(".velnor"))?;
    Ok(dir)
}

fn run(root: &Path, action: &str, extra: &[(&str, &str)]) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("VELNOR_REPO_POLICY_ACTION", action)
        .env("VELNOR_REPO_POLICY_ROOT", root);
    for (key, value) in extra {
        command.env(key, value);
    }
    Ok(command.output()?)
}

#[test]
fn toolchain_specs_parse_the_policy_pins() -> TestResult {
    let dir = root("specs")?;
    fs::write(
        dir.join(".velnor/version-policy.toml"),
        "[tools]\nrust = \"1.99.0\"\nmr-boxington = \"0.9.1\"\nnextest = \"0.9.100\"\nmise = \"2026.11.1\"\n",
    )?;
    let specs = run(&dir, "toolchain-specs", &[])?;
    assert!(specs.status.success(), "{specs:?}");
    assert_eq!(
        String::from_utf8_lossy(&specs.stdout),
        "rust@1.99.0 mr-boxington@0.9.1 aqua:nextest-rs/nextest/cargo-nextest@0.9.100\n"
    );
    let mise = run(&dir, "mise-version", &[])?;
    assert!(mise.status.success(), "{mise:?}");
    assert_eq!(String::from_utf8_lossy(&mise.stdout), "2026.11.1\n");
    Ok(())
}

#[test]
fn missing_pin_fails_the_policy_read() -> TestResult {
    let dir = root("missing-pin")?;
    fs::write(
        dir.join(".velnor/version-policy.toml"),
        "[tools]\nrust = \"1.99.0\"\n",
    )?;
    let output = run(&dir, "toolchain-specs", &[])?;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("version-policy[mr-boxington] missing")
    );
    Ok(())
}

#[test]
fn freshness_without_inventory_fails_without_touching_the_network() -> TestResult {
    let dir = root("no-inventory")?;
    let output = run(
        &dir,
        "freshness",
        &[("VELNOR_REPO_POLICY_CHECK_UPSTREAM", "0")],
    )?;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""check":"inventory-shape""#));
    Ok(())
}

#[test]
fn bad_request_is_rejected_with_the_binary_name() -> TestResult {
    let dir = root("bad-flag")?;
    let output = run(
        &dir,
        "freshness",
        &[("VELNOR_REPO_POLICY_CHECK_UPSTREAM", "maybe")],
    )?;
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "velnor-freshness: internal request failed: VELNOR_REPO_POLICY_CHECK_UPSTREAM must be absent, 0, or 1"
    ));
    Ok(())
}

#[test]
fn missing_request_environment_exits_2() -> TestResult {
    let output = Command::new(BIN).env_clear().output()?;
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    Ok(())
}

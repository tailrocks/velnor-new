// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_velnor-freshness");

fn root(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("freshness-bin-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".velnor")).expect("create fixture root");
    dir
}

fn run(root: &Path, action: &str, extra: &[(&str, &str)]) -> Output {
    let mut command = Command::new(BIN);
    command
        .env_clear()
        .env("VELNOR_REPO_POLICY_ACTION", action)
        .env("VELNOR_REPO_POLICY_ROOT", root);
    for (key, value) in extra {
        command.env(key, value);
    }
    command.output().expect("run velnor-freshness")
}

#[test]
fn toolchain_specs_parse_the_policy_pins() {
    let dir = root("specs");
    fs::write(
        dir.join(".velnor/version-policy.toml"),
        "[tools]\nrust = \"1.99.0\"\nmr-boxington = \"0.9.1\"\nnextest = \"0.9.100\"\nmise = \"2026.11.1\"\n",
    )
    .expect("write policy");
    let specs = run(&dir, "toolchain-specs", &[]);
    assert!(specs.status.success(), "{specs:?}");
    assert_eq!(
        String::from_utf8_lossy(&specs.stdout),
        "rust@1.99.0 mr-boxington@0.9.1 aqua:nextest-rs/nextest/cargo-nextest@0.9.100\n"
    );
    let mise = run(&dir, "mise-version", &[]);
    assert!(mise.status.success(), "{mise:?}");
    assert_eq!(String::from_utf8_lossy(&mise.stdout), "2026.11.1\n");
}

#[test]
fn missing_pin_fails_the_policy_read() {
    let dir = root("missing-pin");
    fs::write(
        dir.join(".velnor/version-policy.toml"),
        "[tools]\nrust = \"1.99.0\"\n",
    )
    .expect("write policy");
    let output = run(&dir, "toolchain-specs", &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("version-policy[mr-boxington] missing")
    );
}

#[test]
fn freshness_without_inventory_fails_without_touching_the_network() {
    let dir = root("no-inventory");
    let output = run(
        &dir,
        "freshness",
        &[("VELNOR_REPO_POLICY_CHECK_UPSTREAM", "0")],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains(r#""check":"inventory-shape""#));
}

#[test]
fn bad_request_is_rejected_with_the_binary_name() {
    let dir = root("bad-flag");
    let output = run(
        &dir,
        "freshness",
        &[("VELNOR_REPO_POLICY_CHECK_UPSTREAM", "maybe")],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "velnor-freshness: internal request failed: VELNOR_REPO_POLICY_CHECK_UPSTREAM must be absent, 0, or 1"
    ));
}

#[test]
fn missing_request_environment_exits_2() {
    let output = Command::new(BIN)
        .env_clear()
        .output()
        .expect("run velnor-freshness");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

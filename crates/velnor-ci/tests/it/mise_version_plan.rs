// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;

fn planned_packages(changed: &str) -> Vec<String> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = std::env::temp_dir().join(format!(
        "velnor-mise-version-plan-{}-{}",
        std::process::id(),
        changed.replace(['/', '.'], "_")
    ));
    fs::create_dir_all(&scratch).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = scratch.join("github-output");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(&repo)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &scratch)
        .env("GITHUB_OUTPUT", &outputs)
        .args(["plan", "--changed", changed])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}"));
    let _ = fs::remove_dir_all(&scratch);
    text.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| key.starts_with("rust_") && key.ends_with("_packages"))
        .flat_map(|(_, value)| value.split_whitespace().map(str::to_owned))
        .collect()
}

#[test]
fn a_change_to_mise_version_runs_the_velnor_gen_tests_that_read_it() {
    let packages = planned_packages(".mise-version");
    assert!(
        packages.iter().any(|package| package == "velnor-gen"),
        "a change to .mise-version selects {packages:?}, not velnor-gen, so \
         crates/velnor-gen/tests/it/mise_version_pin.rs would not run"
    );
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, fs, process::Command};
use velnor_actions_rust::tracked_manifest_in_source;

#[test]
fn tracked_nested_manifest_uses_native_canonical_paths() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    fs::create_dir_all(root.path().join("nested/member"))?;
    fs::write(
        root.path().join("nested/member/Cargo.toml"),
        "[package]\nname='x'\nversion='0.1.0'\n",
    )?;
    fs::create_dir_all(root.path().join("untracked"))?;
    fs::write(root.path().join("untracked/Cargo.toml"), "[workspace]\n")?;
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()?
            .success()
    );
    assert!(
        Command::new("git")
            .args(["add", "nested/member/Cargo.toml"])
            .current_dir(root.path())
            .status()?
            .success()
    );
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root.path())
        .output()?;
    assert!(output.status.success());
    let known: BTreeSet<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8(p.to_vec()))
        .collect::<Result<_, _>>()?;
    assert!(tracked_manifest_in_source(
        root.path(),
        &known,
        "nested/member/Cargo.toml"
    ));
    assert!(!tracked_manifest_in_source(
        root.path(),
        &known,
        "untracked/Cargo.toml"
    ));
    assert!(!tracked_manifest_in_source(
        root.path(),
        &known,
        "nested/missing/Cargo.toml"
    ));
    Ok(())
}

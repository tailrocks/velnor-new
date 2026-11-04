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

#[cfg(unix)]
#[test]
fn metadata_accepts_alias_root_and_rejects_outside_root() -> Result<(), Box<dyn std::error::Error>>
{
    use velnor_actions_rust::parse_metadata_json;
    let temp = tempfile::tempdir()?;
    let real = temp.path().join("real");
    let alias = temp.path().join("alias");
    fs::create_dir_all(real.join("src"))?;
    fs::write(
        real.join("Cargo.toml"),
        "[package]\nname='alias_probe'\nversion='0.1.0'\nedition='2021'\n[workspace]\n",
    )?;
    fs::write(real.join("src/lib.rs"), "pub fn f() {}\n")?;
    std::os::unix::fs::symlink(&real, &alias)?;
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&real)
        .output()?;
    assert!(output.status.success());
    let json = String::from_utf8(output.stdout)?;
    let parsed = parse_metadata_json(&json, &alias, "Cargo.toml", &BTreeSet::new())?;
    assert_eq!(parsed.workspace_root, "");
    assert_eq!(parsed.packages[0].manifest, "Cargo.toml");
    assert!(!parsed.packages[0].external);
    let outside = temp.path().join("outside");
    fs::create_dir(&outside)?;
    assert!(parse_metadata_json(&json, &outside, "Cargo.toml", &BTreeSet::new()).is_err());
    Ok(())
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::prepare::*;
use super::{PreparationBinding, PreparationCheckout};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let result = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()?;
    if !result.status.success() {
        return Err("git failed".into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

fn make_checkout() -> Result<(tempfile::TempDir, PreparationCheckout), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    git(root.path(), &["init", "-q"])?;
    fs::write(root.path().join("Cargo.toml"), "base\n")?;
    fs::write(root.path().join("release-plz.toml"), "")?;
    git(root.path(), &["add", "."])?;
    git(
        root.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "initial",
        ],
    )?;
    let sha = git(root.path(), &["rev-parse", "HEAD"])?;
    let binding = PreparationBinding {
        repository: "owner/project".into(),
        workflow_sha: sha.clone(),
        source_sha: sha,
        run_id: 12,
        run_attempt: 1,
    };
    let checkout = PreparationCheckout::open(
        root.path(),
        binding,
        BTreeSet::from(["Cargo.toml".into()]),
        crate::crate_release_git::ReleaseGit::host_fixture(),
    )?;
    Ok((root, checkout))
}

#[test]
fn git_index_refresh_is_allowed_but_staged_changes_are_rejected()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, checkout) = make_checkout()?;
    fs::write(root.path().join("Cargo.toml"), "base\n")?;
    git(root.path(), &["update-index", "--refresh"])?;
    assert_eq!(checkout.collect()?.files, []);
    let (root, checkout) = make_checkout()?;
    fs::write(root.path().join("Cargo.toml"), "changed\n")?;
    git(root.path(), &["add", "Cargo.toml"])?;
    assert!(checkout.collect().is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn release_tool_must_exist_and_match_its_pin() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("release-plz");
    let digest = super::sha256(b"verified fixture");
    assert_eq!(
        check_pinned_executable(&path, &digest),
        Err("release_pr_tool_invalid")
    );
    fs::write(&path, b"tampered")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    assert_eq!(
        check_pinned_executable(&path, &digest),
        Err("release_pr_tool_invalid")
    );
    fs::write(&path, b"verified fixture")?;
    assert_eq!(check_pinned_executable(&path, &digest), Ok(()));
    Ok(())
}

#[test]
fn rejects_source_and_git_changes_between_snapshots() -> Result<(), Box<dyn std::error::Error>> {
    let (root, checkout) = make_checkout()?;
    fs::write(root.path().join("Cargo.toml"), "update\n")?;
    let path = root.path().join("Cargo.toml");
    assert!(
        checkout
            .collect_with(|| fs::write(&path, "changed again\n").expect("fixture write"))
            .is_err()
    );

    let (root, checkout) = make_checkout()?;
    let config = root.path().join(".git/config");
    let mut bytes = fs::read(&config)?;
    bytes.push(b'\n');
    assert!(
        checkout
            .collect_with(|| fs::write(&config, bytes).expect("fixture write"))
            .is_err()
    );
    Ok(())
}
#[cfg(unix)]
fn update_command(
    source: &Path,
    tools: &Path,
    script: &str,
) -> Result<velnor_actions_mise::command::IsolatedCommand, Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt as _;
    let program = tools.join("release-plz");
    fs::write(&program, script)?;
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))?;
    let args = vec![
        "update".into(),
        "--config".into(),
        source.join("release-plz.toml").into_os_string(),
        "--manifest-path".into(),
        source.join("Cargo.toml").into_os_string(),
    ];
    Ok(velnor_actions_mise::command::IsolatedCommand::repo_task(
        program.to_str().ok_or("program")?,
        args,
        &[],
    )?
    .with_cwd(source.to_path_buf()))
}

#[cfg(unix)]
#[test]
fn updater_success_is_captured_but_failure_never_produces_artifact()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, checkout) = make_checkout()?;
    let tools = tempfile::tempdir()?;
    let cmd = update_command(
        root.path(),
        tools.path(),
        "#!/bin/sh\nprintf 'update\\n' > Cargo.toml\n",
    )?;
    let artifact = execute_preparation_command(checkout, &cmd, std::time::Duration::from_secs(10))?;
    assert_eq!(artifact.files.len(), 1);
    assert_eq!(artifact.files[0].bytes, b"update\n");
    let (root, checkout) = make_checkout()?;
    let tools = tempfile::tempdir()?;
    let cmd = update_command(
        root.path(),
        tools.path(),
        "#!/bin/sh\nprintf 'partial\\n' > Cargo.toml\nexit 1\n",
    )?;
    assert!(
        execute_preparation_command(checkout, &cmd, std::time::Duration::from_secs(10)).is_err()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn updater_children_are_drained_and_timeout_is_not_captured()
-> Result<(), Box<dyn std::error::Error>> {
    let (root, checkout) = make_checkout()?;
    let tools = tempfile::tempdir()?;
    let cmd = update_command(
        root.path(),
        tools.path(),
        "#!/bin/sh\nprintf 'ready\\n' > Cargo.toml\n(sleep 2; printf 'late\\n' > Cargo.toml) >/dev/null 2>&1 &\n",
    )?;
    let artifact = execute_preparation_command(checkout, &cmd, std::time::Duration::from_secs(10))?;
    assert_eq!(artifact.files[0].bytes, b"ready\n");
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert_eq!(fs::read(root.path().join("Cargo.toml"))?, b"ready\n");

    let (root, checkout) = make_checkout()?;
    let tools = tempfile::tempdir()?;
    let cmd = update_command(root.path(), tools.path(), "#!/bin/sh\nsleep 5\n")?;
    assert!(
        execute_preparation_command(checkout, &cmd, std::time::Duration::from_secs(1)).is_err()
    );
    Ok(())
}

#[test]
fn rejects_artifacts_over_byte_budgets() -> Result<(), Box<dyn std::error::Error>> {
    use super::{PreparationArtifact, PreparedFile, verify_preparation_artifact};
    let (_, checkout) = make_checkout()?;
    let binding = checkout.binding.clone();
    let paths = BTreeSet::from([
        "Cargo.toml".to_owned(),
        "a/Cargo.toml".to_owned(),
        "b/Cargo.toml".to_owned(),
        "c/Cargo.toml".to_owned(),
    ]);
    let file = |path: &str, size: usize| {
        let bytes = vec![0; size];
        PreparedFile {
            path: path.to_owned(),
            sha256: super::sha256(&bytes),
            bytes,
        }
    };
    for files in [
        vec![file("Cargo.toml", 8 * 1024 * 1024 + 1)],
        vec![
            file("a/Cargo.toml", 8 * 1024 * 1024),
            file("b/Cargo.toml", 8 * 1024 * 1024),
            file("c/Cargo.toml", 1),
        ],
    ] {
        let artifact = PreparationArtifact {
            schema: 1,
            binding: binding.clone(),
            files,
        };
        let bytes = serde_json::to_vec(&artifact)?;
        assert!(
            verify_preparation_artifact(&bytes, &super::sha256(&bytes), &binding, &paths).is_err()
        );
    }
    let bytes = vec![b' '; 64 * 1024 * 1024 + 1];
    assert!(verify_preparation_artifact(&bytes, &super::sha256(&bytes), &binding, &paths).is_err());
    Ok(())
}

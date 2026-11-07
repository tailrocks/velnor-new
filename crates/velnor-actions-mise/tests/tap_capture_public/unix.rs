// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    env,
    error::Error,
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use velnor_actions_mise::{CheckDeadline, capture_tap_main, read_tap_tree};
#[path = "support.rs"]
mod support;
use support::{
    TempRoot, current_umask, fixture, git_program, install_git_adapter, invoke_git, run_child,
    words,
};

fn physical_mode(path: &Path) -> io::Result<u32> {
    Ok(fs::metadata(path)?.permissions().mode() & 0o7777)
}

fn special_mode_scenario(checkout: &Path, deadline: CheckDeadline) -> Result<(), Box<dyn Error>> {
    let original = capture_tap_main("fixture/tap", checkout, deadline)?;
    let plain = checkout.join("plain.txt");
    fs::set_permissions(&plain, fs::Permissions::from_mode(0o4644))?;
    assert_eq!(physical_mode(&plain)?, 0o4644);
    let captured = capture_tap_main("fixture/tap", checkout, deadline)?;
    assert_eq!(captured.commit_sha, original.commit_sha);
    assert_eq!(fs::read(&plain)?, b"plain tap bytes\n");
    assert_eq!(physical_mode(&plain)?, 0o644);
    let status = invoke_git(
        &git_program()?,
        Some(checkout),
        &words(&["status", "--porcelain=v1", "-z", "--untracked-files=all"]),
    )?;
    assert_eq!(status, Vec::<u8>::new());
    Ok(())
}

fn hardlink_scenario(checkout: &Path, deadline: CheckDeadline) -> Result<(), Box<dyn Error>> {
    capture_tap_main("fixture/tap", checkout, deadline)?;
    let tracked = checkout.join("plain.txt");
    let external = checkout
        .parent()
        .ok_or("checkout has no parent")?
        .join("outside-hardlink.txt");
    fs::hard_link(&tracked, &external)?;
    fs::set_permissions(&tracked, fs::Permissions::from_mode(0o600))?;
    let original_bytes = fs::read(&external)?;
    assert_eq!(physical_mode(&external)?, 0o600);
    let result = capture_tap_main("fixture/tap", checkout, deadline);
    assert_eq!(fs::read(&external)?, original_bytes);
    assert_eq!(physical_mode(&external)?, 0o600);
    if result.is_ok() {
        let external_metadata = fs::metadata(&external)?;
        let tracked_metadata = fs::metadata(&tracked)?;
        assert_ne!(
            (tracked_metadata.dev(), tracked_metadata.ino()),
            (external_metadata.dev(), external_metadata.ino())
        );
        assert_eq!(fs::read(&tracked)?, original_bytes);
        assert_eq!(physical_mode(&tracked)?, 0o644);
        let status = invoke_git(
            &git_program()?,
            Some(checkout),
            &words(&["status", "--porcelain=v1", "-z", "--untracked-files=all"]),
        )?;
        assert_eq!(status, Vec::<u8>::new());
    }
    Ok(())
}

#[test]
fn public_capture_materializes_tracked_modes_under_restrictive_umask() -> Result<(), Box<dyn Error>>
{
    if let Ok(scenario) = env::var("TAP_CAPTURE_CHILD_SCENARIO") {
        return child_scenario(&scenario);
    }
    let parent_umask = current_umask()?;
    let root = TempRoot::new()?;
    let git = git_program()?;
    let valid = fixture(&root.0, &git, false)?;
    let linked = fixture(&root.0, &git, true)?;
    let adapter = install_git_adapter(&root.0)?;
    let executable = env::current_exe()?;

    run_child(
        &executable,
        &adapter,
        &git,
        &valid,
        &root.0.join("checkout-022"),
        "modes",
        "022",
    )?;
    println!("tap_capture_control=pass umask=022");
    run_child(
        &executable,
        &adapter,
        &git,
        &valid,
        &root.0.join("checkout-077"),
        "modes",
        "077",
    )?;
    run_child(
        &executable,
        &adapter,
        &git,
        &valid,
        &root.0.join("checkout-hardlink"),
        "hardlink",
        "077",
    )?;
    run_child(
        &executable,
        &adapter,
        &git,
        &valid,
        &root.0.join("checkout-special"),
        "special",
        "077",
    )?;
    run_child(
        &executable,
        &adapter,
        &git,
        &linked,
        &root.0.join("checkout-link"),
        "link",
        "077",
    )?;
    assert_eq!(current_umask()?, parent_umask);
    Ok(())
}

fn child_scenario(scenario: &str) -> Result<(), Box<dyn Error>> {
    let expected_umask = u32::from_str_radix(&env::var("TAP_CAPTURE_CHILD_UMASK")?, 8)?;
    let commit = env::var("TAP_CAPTURE_EXPECTED_COMMIT")?;
    let checkout = PathBuf::from(env::var_os("TAP_CAPTURE_CHECKOUT").ok_or("missing checkout")?);
    fs::create_dir(&checkout)?;
    fs::set_permissions(&checkout, fs::Permissions::from_mode(0o700))?;
    let deadline = CheckDeadline::after(Duration::from_secs(90))?;

    match scenario {
        "link" => {
            let result = capture_tap_main("fixture/tap", &checkout, deadline);
            assert!(result.is_err());
            let tree = read_tap_tree(
                &env::var_os("TAP_CAPTURE_WORKING_REPOSITORY")
                    .map(PathBuf::from)
                    .ok_or("missing working repository")?,
                &commit,
                deadline,
            );
            assert!(tree.is_err());
            let target = PathBuf::from(
                env::var_os("TAP_CAPTURE_EXTERNAL_TARGET").ok_or("missing external target")?,
            );
            assert_eq!(fs::read(&target)?, b"target bytes remain unchanged\n");
            assert_eq!(physical_mode(&target)?, 0o640);
        }
        "modes" => {
            let captured = capture_tap_main("fixture/tap", &checkout, deadline)?;
            assert_eq!(captured.commit_sha, commit);
            assert_eq!(captured.checkout, checkout);
            let entries = read_tap_tree(&checkout, &commit, deadline)?;
            assert_eq!(captured.tree, entries);
            assert_eq!(captured.tree.len(), 3);
            for (path, mode) in [
                ("nested dir/data.bin", 0o100_644),
                ("plain.txt", 0o100_644),
                ("run me.sh", 0o100_755),
            ] {
                assert!(
                    entries
                        .iter()
                        .any(|entry| entry.path == path && entry.mode == mode)
                );
            }
            for (path, bytes, mode) in [
                ("nested dir/data.bin", &[0, 1, 2, 255][..], 0o644),
                ("plain.txt", &b"plain tap bytes\n"[..], 0o644),
                ("run me.sh", &b"#!/bin/sh\nexit 0\n"[..], 0o755),
            ] {
                let file = checkout.join(path);
                assert_eq!(fs::read(&file)?, bytes);
                assert_eq!(physical_mode(&file)?, mode);
            }
            let git = PathBuf::from(env::var_os("TAP_CAPTURE_REAL_GIT").ok_or("missing git")?);
            let head = invoke_git(&git, Some(&checkout), &words(&["rev-parse", "HEAD"]))?;
            assert_eq!(String::from_utf8(head)?.trim(), commit);
            let status = invoke_git(
                &git,
                Some(&checkout),
                &words(&["status", "--porcelain=v1", "-z", "--untracked-files=all"]),
            )?;
            assert_eq!(status, Vec::<u8>::new());
        }
        "hardlink" => {
            hardlink_scenario(&checkout, deadline)?;
        }
        "special" => special_mode_scenario(&checkout, deadline)?,
        _ => return Err(format!("unknown capture scenario: {scenario}").into()),
    }

    assert_eq!(current_umask()?, expected_umask);
    Ok(())
}

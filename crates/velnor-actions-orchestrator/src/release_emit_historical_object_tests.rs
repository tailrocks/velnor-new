// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[cfg(unix)]
#[test]
fn historical_object_snapshot_rejects_escape_and_ignores_path_git() -> TestResult<()> {
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    if let Some(root) = std::env::var_os("VELNOR_TEST_OBJECT_REPO") {
        let sha = std::env::var("VELNOR_TEST_OBJECT_SHA")?;
        let git = crate::crate_release_git::ReleaseGit::system()?;
        crate::crate_release_qualification_snapshot::snapshot_git_objects(
            Path::new(&root),
            &sha,
            &git,
        )?;
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("repo");
    fs::create_dir(&root)?;
    preview_fixture(&root)?;
    fs::write(root.join("safe.txt"), "historical fixture")?;
    let source = commit(&root, "safe source")?;
    let bin = temp.path().join("bin");
    fs::create_dir(&bin)?;
    let fake = bin.join("git");
    let marker = temp.path().join("fake-ran");
    fs::write(
        &fake,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )?;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700))?;
    let child = Command::new(std::env::current_exe()?)
        .env_clear()
        .env("PATH", &bin)
        .env("VELNOR_TEST_OBJECT_REPO", &root)
        .env("VELNOR_TEST_OBJECT_SHA", &source)
        .args([
            "--exact",
            crate::libtest_case::name(concat!(
                module_path!(),
                "::historical_object_snapshot_rejects_escape_and_ignores_path_git"
            )),
            "--nocapture",
        ])
        .output()?;
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    crate::libtest_case::assert_one_passed(&child);
    assert!(!marker.exists());
    symlink("../../outside", root.join("escape"))?;
    let hostile = commit(&root, "escaping link")?;
    let git = crate::crate_release_git::ReleaseGit::system()?;
    assert!(
        crate::crate_release_qualification_snapshot::snapshot_git_objects(&root, &hostile, &git)
            .is_err()
    );
    assert!(!temp.path().join("outside").exists());
    Ok(())
}

//! The patch filter, executed: publish against crafted attack patches, and
//! prepare's drop mode against a staged index.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};

use super::exec_support::{Sandbox, TestResult, text};

/// The workflow must refuse the patch before any token exists, push nothing
/// and leave the checkout alone.
fn assert_publish_rejects(sandbox: &Sandbox, code: &str) -> TestResult {
    let head = sandbox.git(&["rev-parse", "HEAD"])?;
    let run = sandbox.pipeline()?;
    let log = run.log();
    assert!(run.publish.is_none(), "verify accepted the patch:\n{log}");
    assert!(!sandbox.token_minted(), "a token was minted:\n{log}");
    assert!(
        log.contains(&format!("::error::{code}: ")),
        "{code}:\n{log}"
    );
    assert_eq!(sandbox.pushed_branch()?, None, "pushed:\n{log}");
    assert_eq!(sandbox.git(&["rev-parse", "HEAD"])?, head);
    assert_eq!(
        sandbox.git(&["status", "--porcelain", "--untracked-files=all"])?,
        "",
        "a rejected patch touched the checkout:\n{log}"
    );
    assert!(
        !sandbox.calls()?.contains("gh pr"),
        "a rejected patch reached the GitHub CLI"
    );
    Ok(())
}

#[test]
fn publish_ships_an_owned_patch() -> TestResult {
    let sandbox = Sandbox::new("owned")?;
    sandbox.write("Cargo.lock", "version = 4\n# bumped\n")?;
    sandbox.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n")?;
    sandbox.capture_patch(&[])?;
    let run = sandbox.pipeline()?;
    let publish = run.publish.as_ref().ok_or_else(|| run.log())?;
    assert!(publish.status.success(), "{}", run.log());
    let pushed = sandbox.pushed_branch()?.ok_or("nothing pushed")?;
    assert_eq!(pushed.len(), 40);
    assert!(sandbox.calls()?.contains("gh pr create"));
    Ok(())
}

#[test]
fn rename_of_a_foreign_file_into_an_owned_path_is_rejected() -> TestResult {
    // The review's reproduction: `src/main.rs -> a/Cargo.toml` passed the
    // destination-only filter and `git apply` staged the rename.
    let sandbox = Sandbox::new("rename")?;
    fs::create_dir_all(sandbox.repo.join("a"))?;
    fs::rename(
        sandbox.repo.join("src/main.rs"),
        sandbox.repo.join("a/Cargo.toml"),
    )?;
    sandbox.capture_patch(&["-M"])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_rename_rejected")
}

#[test]
fn the_same_move_without_rename_detection_is_rejected_as_a_foreign_deletion() -> TestResult {
    let sandbox = Sandbox::new("move")?;
    fs::create_dir_all(sandbox.repo.join("a"))?;
    fs::rename(
        sandbox.repo.join("src/main.rs"),
        sandbox.repo.join("a/Cargo.toml"),
    )?;
    sandbox.capture_patch(&["--no-renames"])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_path_rejected")
}

#[test]
fn copy_of_a_foreign_file_is_rejected() -> TestResult {
    let sandbox = Sandbox::new("copy")?;
    fs::create_dir_all(sandbox.repo.join("x"))?;
    fs::copy(
        sandbox.repo.join("deny.toml"),
        sandbox.repo.join("x/Cargo.toml"),
    )?;
    sandbox.capture_patch(&["-C", "--find-copies-harder"])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_rename_rejected")
}

#[test]
fn deleting_a_foreign_file_is_rejected() -> TestResult {
    let sandbox = Sandbox::new("delete-foreign")?;
    fs::remove_file(sandbox.repo.join("deny.toml"))?;
    sandbox.capture_patch(&[])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_path_rejected")
}

#[test]
fn deleting_an_owned_lockfile_is_rejected() -> TestResult {
    let sandbox = Sandbox::new("delete-owned")?;
    fs::remove_file(sandbox.repo.join("Cargo.lock"))?;
    sandbox.capture_patch(&[])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_delete_rejected")
}

#[test]
fn a_release_that_retires_a_generated_workflow_may_delete_it() -> TestResult {
    let sandbox = Sandbox::new("delete-generated")?;
    sandbox.write(".github/workflows/retired.yml", "name: Retired\n")?;
    sandbox.git(&["add", "-A"])?;
    sandbox.git(&["commit", "--quiet", "-m", "retired workflow"])?;
    sandbox.release("0.2.0")?;
    sandbox.bump_velnor("0.2.0")?;
    fs::remove_file(sandbox.repo.join(".github/workflows/retired.yml"))?;
    sandbox.capture_patch(&[])?;
    let run = sandbox.pipeline()?;
    let publish = run.publish.as_ref().ok_or_else(|| run.log())?;
    assert!(publish.status.success(), "{}", run.log());
    assert!(
        sandbox
            .pushed_file(".github/workflows/retired.yml")
            .is_err()
    );
    Ok(())
}

#[test]
fn symlink_replacing_an_owned_file_is_rejected() -> TestResult {
    let sandbox = Sandbox::new("symlink-swap")?;
    fs::remove_file(sandbox.repo.join("mise.toml"))?;
    symlink("/etc/passwd", sandbox.repo.join("mise.toml"))?;
    sandbox.capture_patch(&[])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_symlink_rejected")
}

#[test]
fn new_symlink_at_an_owned_path_is_rejected() -> TestResult {
    let sandbox = Sandbox::new("symlink-new")?;
    fs::create_dir_all(sandbox.repo.join("x"))?;
    symlink("../deny.toml", sandbox.repo.join("x/Cargo.toml"))?;
    sandbox.capture_patch(&[])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_symlink_rejected")
}

#[test]
fn executable_bit_on_an_owned_file_is_rejected() -> TestResult {
    let sandbox = Sandbox::new("chmod")?;
    fs::set_permissions(
        sandbox.repo.join("Cargo.toml"),
        fs::Permissions::from_mode(0o755),
    )?;
    sandbox.capture_patch(&[])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_mode_rejected")
}

#[test]
fn new_executable_file_is_rejected() -> TestResult {
    let sandbox = Sandbox::new("new-exec")?;
    sandbox.write(".github/workflows/run.sh", "#!/bin/sh\n")?;
    fs::set_permissions(
        sandbox.repo.join(".github/workflows/run.sh"),
        fs::Permissions::from_mode(0o755),
    )?;
    sandbox.capture_patch(&[])?;
    assert_publish_rejects(&sandbox, "upgrade_patch_mode_rejected")
}

#[test]
fn binary_patches_are_rejected_in_both_forms() -> TestResult {
    for (name, args) in [("binary-full", &["--binary"][..]), ("binary-stub", &[][..])] {
        let sandbox = Sandbox::new(name)?;
        fs::write(
            sandbox.repo.join(".github/logo.png"),
            [0x89, b'P', b'N', b'G', 0, 1, 2, 0, 255],
        )?;
        sandbox.capture_patch(args)?;
        assert_publish_rejects(&sandbox, "upgrade_patch_binary_rejected")?;
    }
    Ok(())
}

#[test]
fn traditional_patch_sections_are_rejected() -> TestResult {
    let sandbox = Sandbox::new("traditional")?;
    sandbox.write("Cargo.lock", "version = 4\n# bumped\n")?;
    sandbox.capture_patch(&[])?;
    let patch = sandbox.work.join("upgrade.patch");
    let mut text = fs::read_to_string(&patch)?;
    text.push_str("--- a/Cargo.toml\n+++ b/Cargo.toml\n@@ -1 +1 @@\n-[package]\n+[workspace]\n");
    fs::write(&patch, text)?;
    assert_publish_rejects(&sandbox, "upgrade_patch_malformed")
}

#[test]
fn dot_dot_and_dot_git_paths_are_rejected_by_the_filter_itself() -> TestResult {
    for (name, path) in [
        ("dotdot", "x/../Cargo.toml"),
        ("dotgit", ".GIT/hooks/Cargo.toml"),
        ("absolute", "/tmp/Cargo.toml"),
    ] {
        let sandbox = Sandbox::new(name)?;
        let patch = format!(
            "diff --git a/{path} b/{path}\nnew file mode 100644\nindex 0000000..e69de29\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+x\n"
        );
        fs::write(sandbox.work.join("upgrade.patch"), patch)?;
        fs::write(sandbox.work.join("summary.md"), "- fixture\n")?;
        let output = sandbox.run(
            &format!(
                "{}\nvelnor_upgrade_check_patch \"$VELNOR_UPGRADE_DIR/upgrade.patch\"",
                super::FILTER
            ),
            &[],
        )?;
        let log = text(&output);
        assert!(!output.status.success(), "{path}:\n{log}");
        assert!(
            log.contains("::error::upgrade_patch_path_rejected: ")
                || log.contains("::error::upgrade_patch_malformed: "),
            "{path}:\n{log}"
        );
    }
    Ok(())
}

#[test]
fn prepare_drop_mode_unstages_every_violation_and_keeps_the_rest() -> TestResult {
    let sandbox = Sandbox::new("drop")?;
    sandbox.write("Cargo.lock", "version = 4\n# bumped\n")?;
    sandbox.write("src/lib.rs", "pub fn injected() {}\n")?;
    fs::remove_file(sandbox.repo.join("deny.toml"))?;
    fs::remove_file(sandbox.repo.join("mise.toml"))?;
    symlink("/etc/passwd", sandbox.repo.join("mise.toml"))?;
    fs::set_permissions(
        sandbox.repo.join("Cargo.toml"),
        fs::Permissions::from_mode(0o755),
    )?;
    fs::write(sandbox.repo.join(".github/logo.png"), [0u8, 159, 146, 150])?;
    let output = sandbox.run(
        &format!(
            "set -euo pipefail\n{}\ngit add -A\nvelnor_upgrade_check_index drop\necho STAGED\ngit diff --cached --name-only --no-renames",
            super::FILTER
        ),
        &[],
    )?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    let (dropped, staged) = log.split_once("STAGED\n").ok_or("no marker")?;
    for line in [
        "upgrade_patch_path_rejected src/lib.rs",
        "upgrade_patch_path_rejected deny.toml",
        "upgrade_patch_symlink_rejected mise.toml",
        "upgrade_patch_mode_rejected Cargo.toml",
        "upgrade_patch_binary_rejected .github/logo.png",
    ] {
        assert!(dropped.contains(line), "{line}:\n{log}");
    }
    assert_eq!(staged.trim(), "Cargo.lock", "{log}");
    Ok(())
}

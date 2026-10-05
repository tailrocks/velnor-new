//! Clone authority is separate from an already initialized fixture checkout.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use super::git_clone_fixture::clone_fixture;
use crate::impl_common::{TestResult, git, git_line, snapshot};

const CHILD: &str = "VELNOR_CLONE_ISOLATION_CHILD";

fn seed(root: &Path, branch: &str) -> TestResult {
    git(&["init", "-b", branch], root)?;
    git(&["config", "user.email", "test@example.invalid"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    fs::write(root.join("seed.txt"), "original\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "seed"], root)?;
    Ok(())
}

fn clone_child() -> TestResult {
    let source = std::env::var_os("VELNOR_CLONE_SOURCE").ok_or("missing clone source")?;
    let destination = std::env::var_os("VELNOR_CLONE_DESTINATION").ok_or("missing destination")?;
    let source = Path::new(&source);
    let destination = Path::new(&destination);
    let head = git_line(&["rev-parse", "HEAD"], source)?;
    assert!(std::env::var_os("GIT_DIR").is_some());
    let output = clone_fixture(source, destination)?.output()?;
    assert!(output.status.success(), "{:?}", output.stderr);
    assert_eq!(git_line(&["rev-parse", "HEAD"], destination)?, head);
    assert_eq!(
        git_line(&["symbolic-ref", "refs/remotes/origin/HEAD"], destination)?,
        "refs/remotes/origin/upstream-main"
    );
    assert_eq!(fs::read(destination.join("seed.txt"))?, b"original\n");
    assert!(!destination.join(".git/hooks/post-checkout").exists());
    assert!(!destination.join("hook-ran").exists());
    git(
        &["config", "user.email", "test@example.invalid"],
        destination,
    )?;
    git(&["config", "user.name", "Test"], destination)?;
    fs::write(destination.join("seed.txt"), "clone only\n")?;
    git(&["add", "."], destination)?;
    git(&["commit", "-m", "clone"], destination)?;
    assert_ne!(git_line(&["rev-parse", "HEAD"], destination)?, head);
    assert!(!destination.join("hook-ran").exists());
    Ok(())
}

fn poison(root: &Path) -> TestResult {
    for name in ["hooks", "template/hooks"] {
        fs::create_dir_all(root.join(name))?;
        for hook in ["post-checkout", "pre-commit"] {
            let path = root.join(name).join(hook);
            fs::write(&path, "#!/bin/sh\n: > hook-ran\nexit 97\n")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    fs::write(
        root.join("config"),
        format!(
            "[core]\nhooksPath = {}\n[commit]\ngpgsign = true\n",
            root.join("hooks").display()
        ),
    )?;
    Ok(())
}

#[test]
fn clone_ignores_inherited_git_authority() -> TestResult {
    if std::env::var_os(CHILD).is_some() {
        return clone_child();
    }
    let source = TempDir::new()?;
    let victim = TempDir::new()?;
    let parent = TempDir::new()?;
    let hostile = TempDir::new()?;
    seed(source.path(), "upstream-main")?;
    seed(victim.path(), "victim-main")?;
    fs::write(victim.path().join("seed.txt"), "staged\n")?;
    git(&["add", "."], victim.path())?;
    fs::write(victim.path().join("seed.txt"), "unstaged\n")?;
    poison(hostile.path())?;
    let before = snapshot(victim.path())?;
    let source_before = snapshot(source.path())?;
    let destination = parent.path().join("clone");
    let output =
        hostile_child(source.path(), &destination, victim.path(), hostile.path())?.output()?;
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    assert_eq!(snapshot(victim.path())?, before, "victim metadata changed");
    assert_eq!(
        snapshot(source.path())?,
        source_before,
        "clone source changed"
    );
    assert!(!parent.path().join(".git").exists());
    Ok(())
}

fn hostile_child(
    source: &Path,
    destination: &Path,
    victim: &Path,
    hostile: &Path,
) -> Result<Command, Box<dyn std::error::Error>> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args([
            "impl_config_internal::clone_tests::clone_ignores_inherited_git_authority",
            "--exact",
        ])
        .env(CHILD, "1")
        .env("VELNOR_CLONE_SOURCE", source)
        .env("VELNOR_CLONE_DESTINATION", destination)
        .env("GIT_DIR", victim.join(".git"))
        .env("GIT_COMMON_DIR", victim.join(".git"))
        .env("GIT_WORK_TREE", victim)
        .env("GIT_INDEX_FILE", victim.join(".git/index"))
        .env("GIT_OBJECT_DIRECTORY", victim.join(".git/objects"))
        .env(
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            victim.join(".git/objects"),
        )
        .env("GIT_CONFIG", victim.join(".git/config"))
        .env("GIT_CONFIG_SYSTEM", hostile.join("config"))
        .env("GIT_CONFIG_GLOBAL", hostile.join("config"))
        .env("GIT_TEMPLATE_DIR", hostile.join("template"))
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_0", hostile.join("hooks"))
        .env("GIT_CONFIG_PARAMETERS", "'commit.gpgsign=true'")
        .env("HOME", hostile)
        .env("XDG_CONFIG_HOME", hostile);
    Ok(command)
}

#[test]
fn clone_rejects_existing_destinations() -> TestResult {
    let source = TempDir::new()?;
    let parent = TempDir::new()?;
    seed(source.path(), "upstream-main")?;
    for name in ["empty", "populated"] {
        let destination = parent.path().join(name);
        fs::create_dir(&destination)?;
        if name == "populated" {
            fs::write(destination.join("keep.txt"), "keep\n")?;
        }
        let before = snapshot(&destination)?;
        assert!(clone_fixture(source.path(), &destination).is_err());
        assert_eq!(snapshot(&destination)?, before);
    }
    let file = parent.path().join("file");
    fs::write(&file, "keep\n")?;
    assert!(clone_fixture(source.path(), &file).is_err());
    assert_eq!(fs::read(file)?, b"keep\n");
    Ok(())
}

#[test]
#[cfg(unix)]
fn clone_rejects_live_and_dangling_symlink_destinations() -> TestResult {
    let source = TempDir::new()?;
    let parent = TempDir::new()?;
    let target = TempDir::new()?;
    seed(source.path(), "upstream-main")?;
    fs::write(target.path().join("keep.txt"), "keep\n")?;
    let before = snapshot(target.path())?;
    for (name, target_path) in [
        ("live", target.path().to_path_buf()),
        ("dangling", parent.path().join("absent")),
    ] {
        let destination = parent.path().join(name);
        std::os::unix::fs::symlink(&target_path, &destination)?;
        assert!(clone_fixture(source.path(), &destination).is_err());
        assert_eq!(fs::read_link(destination)?, target_path);
    }
    assert_eq!(snapshot(target.path())?, before);
    assert!(!parent.path().join("absent").exists());
    Ok(())
}

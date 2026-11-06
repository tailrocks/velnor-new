// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

type TestResult<T> = Result<T, Box<dyn Error>>;

fn git(root: &Path, args: &[&str]) -> TestResult<Output> {
    let output = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Alexey Zhokhov",
            "-c",
            "user.email=alexey@zhokhov.com",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("GIT_NO_REPLACE_OBJECTS")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(output)
}

fn build(root: &Path, source: &str, target: &Path) -> TestResult<Output> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    Ok(Command::new(cargo)
        .args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(root.join("crates/velnor-runner/Cargo.toml"))
        .args(["-p", "velnor-runner-host", "--lib"])
        .env("CARGO_TARGET_DIR", target)
        .env("VELNOR_RUNNER_BUILD_SOURCE_SHA", source)
        .env("VELNOR_RUNNER_BUILD_HELPER_SHA256", "a".repeat(64))
        .env_remove("GIT_NO_REPLACE_OBJECTS")
        .output()?)
}

fn init_git(root: &Path) -> TestResult<()> {
    if !Command::new("/usr/bin/git")
        .args(["init", "-q"])
        .arg(root)
        .status()?
        .success()
    {
        return Err("git init failed".into());
    }
    Ok(())
}

#[test]
fn host_build_binds_embedded_runner_lock_to_exact_commit() -> TestResult<()> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    let original_host = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = original_host
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace")?;
    let destination = root.join("crates/velnor-runner");
    fs::create_dir_all(&destination)?;
    for name in ["Cargo.toml", "Cargo.lock", "crates"] {
        let status = Command::new("cp")
            .arg("-R")
            .arg(workspace.join(name))
            .arg(&destination)
            .status()?;
        if !status.success() {
            return Err("copy fixture failed".into());
        }
    }
    fs::create_dir(root.join(".velnor"))?;
    let lock = root.join(".velnor/runner.lock");
    fs::copy(workspace.join("../../.velnor/runner.lock"), &lock)?;
    init_git(root)?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "fixture source",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let source = String::from_utf8(git(root, &["rev-parse", "HEAD"])?.stdout)?
        .trim()
        .to_owned();
    let original = fs::read(&lock)?;
    let target = fixture.path().join("cargo-target");
    let positive = build(root, &source, &target)?;
    assert!(
        positive.status.success(),
        "{}",
        String::from_utf8_lossy(&positive.stderr)
    );
    fs::write(&lock, b"changed lock\n")?;
    let dirty = build(root, &source, &target)?;
    assert!(!dirty.status.success(), "accepted changed source lock");
    fs::write(&lock, &original)?;
    let absent = build(root, &"f".repeat(40), &target)?;
    assert!(!absent.status.success(), "accepted nonexistent source");
    fs::write(&lock, b"changed lock\n")?;
    git(root, &["add", ".velnor/runner.lock"])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "different lock",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let replacement = String::from_utf8(git(root, &["rev-parse", "HEAD"])?.stdout)?
        .trim()
        .to_owned();
    git(root, &["replace", &source, &replacement])?;
    let forged = build(root, &source, &target)?;
    assert!(!forged.status.success(), "accepted replacement-ref lock");
    verify_missing_promisor_blob(root, &source, &target, &original)?;
    verify_git_exec_path_cannot_replace_child(root, &source, &target)?;
    Ok(())
}

fn verify_missing_promisor_blob(
    root: &Path,
    source: &str,
    target: &Path,
    lock: &[u8],
) -> TestResult<()> {
    use std::os::unix::fs::PermissionsExt;
    git(root, &["replace", "-d", source])?;
    fs::write(root.join(".velnor/runner.lock"), lock)?;
    let spec = format!("{source}:.velnor/runner.lock");
    let blob = String::from_utf8(git(root, &["rev-parse", &spec])?.stdout)?;
    let blob = blob.trim();
    let object = root.join(".git/objects").join(&blob[..2]).join(&blob[2..]);
    assert!(object.is_file(), "fixture blob must exist before removal");
    fs::remove_file(&object)?;
    git(root, &["config", "extensions.partialClone", "fixture"])?;
    git(root, &["config", "remote.fixture.promisor", "true"])?;
    git(root, &["config", "remote.fixture.url", "fixture::ignored"])?;
    git(root, &["config", "protocol.fixture.allow", "always"])?;
    let bin = root.join("remote-bin");
    fs::create_dir(&bin)?;
    let remote = bin.join("git-remote-fixture");
    fs::write(
        &remote,
        "#!/bin/sh\nprintf fetched > \"$FIXTURE_REMOTE_MARKER\"\nexit 1\n",
    )?;
    fs::set_permissions(&remote, fs::Permissions::from_mode(0o755))?;
    let path = format!("{}:{}", bin.display(), std::env::var("PATH")?);
    let marker = root.join("remote-invoked");
    let probe = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "blob", &spec])
        .env("PATH", &path)
        .env("FIXTURE_REMOTE_MARKER", &marker)
        .env("GIT_NO_LAZY_FETCH", "0")
        .env_remove("GIT_ALLOW_PROTOCOL")
        .output()?;
    assert!(
        !probe.status.success(),
        "fixture remote unexpectedly succeeded"
    );
    assert!(
        marker.is_file(),
        "unguarded Git did not invoke fixture remote"
    );
    fs::remove_file(&marker)?;
    assert!(!object.exists(), "fixture must still lack its lock object");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let build = Command::new(cargo)
        .args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(root.join("crates/velnor-runner/Cargo.toml"))
        .args(["-p", "velnor-runner-host", "--lib"])
        .env("CARGO_TARGET_DIR", target)
        .env("VELNOR_RUNNER_BUILD_SOURCE_SHA", source)
        .env("VELNOR_RUNNER_BUILD_HELPER_SHA256", "a".repeat(64))
        .env("PATH", path)
        .env("FIXTURE_REMOTE_MARKER", &marker)
        .output()?;
    assert!(
        !build.status.success(),
        "accepted unavailable committed lock"
    );
    assert!(!marker.exists(), "host build attempted a promisor fetch");
    Ok(())
}

fn verify_git_exec_path_cannot_replace_child(
    root: &Path,
    source: &str,
    target: &Path,
) -> TestResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let executable_dir = root.join("substituted-git");
    fs::create_dir(&executable_dir)?;
    let git = executable_dir.join("git");
    fs::write(
        &git,
        "#!/bin/sh\nprintf invoked > \"$FIXTURE_EXEC_MARKER\"\nexit 1\n",
    )?;
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755))?;
    let marker = root.join("git-child-invoked");
    let spec = format!("{source}:.velnor/runner.lock");
    let probe = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "blob", &spec])
        .env("GIT_EXEC_PATH", &executable_dir)
        .env("FIXTURE_EXEC_MARKER", &marker)
        .env("GIT_NO_LAZY_FETCH", "0")
        .env("GIT_ALLOW_PROTOCOL", "")
        .output()?;
    assert!(
        !probe.status.success(),
        "fixture child unexpectedly succeeded"
    );
    assert!(
        marker.is_file(),
        "unguarded Git did not invoke substituted child"
    );
    fs::remove_file(&marker)?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let build = Command::new(cargo)
        .args(["build", "--offline", "--locked", "--manifest-path"])
        .arg(root.join("crates/velnor-runner/Cargo.toml"))
        .args(["-p", "velnor-runner-host", "--lib"])
        .env("CARGO_TARGET_DIR", target)
        .env("VELNOR_RUNNER_BUILD_SOURCE_SHA", source)
        .env("VELNOR_RUNNER_BUILD_HELPER_SHA256", "b".repeat(64))
        .env("GIT_EXEC_PATH", &executable_dir)
        .env("FIXTURE_EXEC_MARKER", &marker)
        .output()?;
    assert!(
        !build.status.success(),
        "accepted unavailable committed lock"
    );
    assert!(!marker.exists(), "host build invoked substituted Git child");
    Ok(())
}

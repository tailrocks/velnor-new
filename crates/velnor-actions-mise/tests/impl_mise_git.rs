//! Git verb allowlist cases, including live read-only executions.
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;
use velnor_actions_mise::{
    ALLOWED_GIT_VERBS, GitRequest, IsolatedCommand, MiseError, is_allowed_git_verb,
};

fn scratch_dir(test: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("velnor-mise-{test}-{}", std::process::id()));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) | Err(_) => {}
    }
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

fn run_git(dir: &Path, args: &[&str], optional_locks: &str) -> Result<Vec<u8>, String> {
    let path = env::var_os("PATH").ok_or_else(|| "PATH missing".to_owned())?;
    let output = StdCommand::new(git_executable()?)
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", dir)
        .env("GIT_CONFIG_GLOBAL", dir.join("empty-gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", optional_locks)
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(output.stdout)
}

fn git_executable() -> Result<OsString, String> {
    let path = env::var_os("PATH").ok_or_else(|| "PATH missing".to_owned())?;
    env::split_paths(&path)
        .map(|directory| directory.join("git"))
        .find(|candidate| candidate.is_file())
        .map(std::path::PathBuf::into_os_string)
        .ok_or_else(|| "git executable missing".to_owned())
}

fn git_fixture() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let dir = scratch_dir("git-optional-locks")?;
    run_git(&dir, &["init", "--quiet"], "1")?;
    let tracked = dir.join("tracked");
    fs::write(&tracked, b"unchanged\n").map_err(|error| error.to_string())?;
    run_git(&dir, &["add", "--", "tracked"], "1")?;
    run_git(
        &dir,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
        "1",
    )?;
    Ok((dir.clone(), tracked, dir.join(".git/index")))
}

fn parent_with_hostile_git_locks(dir: &Path) -> Result<Vec<(OsString, OsString)>, String> {
    let path = env::var_os("PATH").ok_or_else(|| "PATH missing".to_owned())?;
    Ok(vec![
        (OsString::from("PATH"), path),
        (OsString::from("HOME"), dir.as_os_str().to_owned()),
        (
            OsString::from("GIT_CONFIG_GLOBAL"),
            dir.join("empty-gitconfig").into_os_string(),
        ),
        (OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("1")),
        (OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("1")),
    ])
}

fn run_git_with_discovery_env(
    request: &GitRequest,
    args: &[&str],
    dir: &Path,
    parent: &[(OsString, OsString)],
) -> Result<std::process::Output, String> {
    let command = request.command_in(dir);
    StdCommand::new(command.program())
        .args(args)
        .current_dir(dir)
        .env_clear()
        .envs(command.spawn_env(parent))
        .output()
        .map_err(|error| error.to_string())
}

fn bump_mtime(path: &Path) -> Result<(), String> {
    let modified = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| error.to_string())?
        .checked_add(Duration::from_secs(2))
        .ok_or_else(|| "tracked mtime overflow".to_owned())?;
    fs::File::open(path)
        .and_then(|file| file.set_modified(modified))
        .map_err(|error| error.to_string())
}

fn run_repo_task_git_add(dir: &Path) -> Result<(), String> {
    let git = git_executable()?.to_string_lossy().into_owned();
    let config = dir.join("empty-gitconfig").into_os_string();
    let declared = [
        (OsString::from("HOME"), dir.as_os_str().to_owned()),
        (OsString::from("GIT_CONFIG_GLOBAL"), config),
        (OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("1")),
    ];
    let write = IsolatedCommand::repo_task(
        &git,
        vec![
            OsString::from("-C"),
            dir.as_os_str().to_owned(),
            OsString::from("add"),
            OsString::from("--"),
            OsString::from("tracked"),
        ],
        &declared,
    )
    .map_err(|error| error.to_string())?
    .with_env(&[(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("1"))])
    .map_err(|error| error.to_string())?;
    let output = write.run().map_err(|error| error.to_string())?;
    if !output.success {
        return Err("explicit git add must succeed".to_owned());
    }
    Ok(())
}

#[test]
fn allowlist_accepts_exactly_six_verbs() {
    assert_eq!(
        ALLOWED_GIT_VERBS,
        [
            "rev-parse",
            "ls-files",
            "diff",
            "show",
            "config",
            "merge-base"
        ]
    );
    for verb in ALLOWED_GIT_VERBS {
        assert!(is_allowed_git_verb(verb), "verb must be allowed: {verb}");
        assert!(
            GitRequest::new(verb, Vec::new()).is_ok(),
            "verb must construct: {verb}"
        );
    }
    assert_eq!(GitRequest::rev_parse(Vec::new()).verb(), "rev-parse");
    assert_eq!(GitRequest::ls_files(Vec::new()).verb(), "ls-files");
    assert_eq!(GitRequest::diff(Vec::new()).verb(), "diff");
    assert_eq!(GitRequest::show(Vec::new()).verb(), "show");
    assert_eq!(GitRequest::config(Vec::new()).verb(), "config");
    assert_eq!(GitRequest::merge_base(Vec::new()).verb(), "merge-base");
}

#[test]
fn allowlist_rejects_mutating_and_info_verbs() {
    for verb in [
        "checkout",
        "status",
        "push",
        "pull",
        "log",
        "branch",
        "clone",
        "init",
        "add",
        "--version",
        "REV-PARSE",
        "",
    ] {
        assert!(!is_allowed_git_verb(verb), "verb must be denied: {verb}");
        assert!(
            matches!(
                GitRequest::new(verb, Vec::new()),
                Err(MiseError::GitVerbRejected { .. })
            ),
            "verb must fail typed: {verb}"
        );
    }
}

#[test]
fn git_argv_runs_git_directly() {
    let request = GitRequest::rev_parse(vec![OsString::from("--show-toplevel")]);
    assert_eq!(
        request.argv(),
        vec![
            OsString::from("git"),
            OsString::from("rev-parse"),
            OsString::from("--show-toplevel"),
        ]
    );
    let command = request.command();
    assert_eq!(command.argv(), request.argv());
    assert_eq!(command.program(), "git");
}

#[test]
fn git_command_in_records_cwd() {
    let request = GitRequest::diff(vec![OsString::from("--name-only")]);
    let cwd = std::env::temp_dir();
    let command = request.command_in(&cwd);
    assert_eq!(command.cwd(), Some(&cwd));
    assert_eq!(command.argv(), request.argv());
}

#[test]
fn discovery_git_owns_optional_lock_control() -> Result<(), String> {
    for verb in ALLOWED_GIT_VERBS {
        let request = GitRequest::new(verb, Vec::new()).map_err(|error| error.to_string())?;
        let command = request.command();
        assert!(
            command
                .full_env()
                .contains(&(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("0")))
        );
        let parent = [(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("1"))];
        assert_eq!(
            command
                .spawn_env(&parent)
                .iter()
                .rev()
                .find(|(key, _)| key == "GIT_OPTIONAL_LOCKS"),
            Some(&(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("0")))
        );
        assert!(matches!(
            command.with_env(&[(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("1"))]),
            Err(MiseError::InvalidStepInput { field, .. }) if field == "GIT_OPTIONAL_LOCKS"
        ));
    }
    Ok(())
}

#[test]
fn live_git_status_preserves_index_and_repo_task_writes_remain_explicit() -> Result<(), String> {
    let (dir, tracked, index) = git_fixture()?;
    let before = fs::read(&index).map_err(|error| error.to_string())?;
    bump_mtime(&tracked)?;
    // Status is intentionally outside GitRequest's allowlist; run this read
    // under the shared discovery environment to prove its optional-lock effect.
    let request = GitRequest::rev_parse(Vec::new());
    let output = run_git_with_discovery_env(
        &request,
        &["status", "--porcelain"],
        &dir,
        &parent_with_hostile_git_locks(&dir)?,
    )?;
    assert!(
        output.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(&index).map_err(|error| error.to_string())?, before);

    run_git(&dir, &["status", "--porcelain"], "1")?;
    let refreshed = fs::read(&index).map_err(|error| error.to_string())?;
    assert_ne!(
        refreshed, before,
        "control Git status must refresh the index"
    );
    fs::write(&tracked, b"changed\n").map_err(|error| error.to_string())?;
    run_repo_task_git_add(&dir)?;
    assert_ne!(
        fs::read(&index).map_err(|error| error.to_string())?,
        refreshed,
        "explicit Git write task must update the index"
    );
    Ok(())
}

#[test]
fn live_git_rev_parse_reports_typed_success() -> Result<(), String> {
    let dir = scratch_dir("git-repo")?;
    let git_dir = dir.join(".git");
    std::fs::create_dir_all(git_dir.join("objects")).map_err(|err| err.to_string())?;
    std::fs::create_dir_all(git_dir.join("refs").join("heads")).map_err(|err| err.to_string())?;
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n")
        .map_err(|err| err.to_string())?;
    let request = GitRequest::rev_parse(vec![OsString::from("--show-toplevel")]);
    let output = request.run_in(&dir).map_err(|err| err.to_string())?;
    assert!(output.success, "rev-parse must succeed inside a repository");
    assert_eq!(output.code, Some(0));
    let text = output.stdout_text("git").map_err(|err| err.to_string())?;
    assert!(
        !text.trim().is_empty(),
        "rev-parse must print the top level"
    );
    Ok(())
}

#[test]
fn live_git_outside_repo_reports_typed_failure() -> Result<(), String> {
    let dir = scratch_dir("git-outside-repo")?;
    let request = GitRequest::rev_parse(vec![OsString::from("--show-toplevel")]);
    let output = request.run_in(&dir).map_err(|err| err.to_string())?;
    assert!(!output.success, "rev-parse must fail outside a repository");
    assert!(matches!(
        output.require_success("git"),
        Err(MiseError::NonZeroExit { .. })
    ));
    assert!(
        !output.stderr.is_empty(),
        "failure must capture stderr bytes"
    );
    Ok(())
}

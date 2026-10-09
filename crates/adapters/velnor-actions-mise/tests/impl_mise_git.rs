//! Git verb allowlist cases, including live read-only executions.
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};
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

fn git_executable() -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").ok_or_else(|| "PATH is unset".to_owned())?;
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "git executable was not found in PATH".to_owned())
}

fn git_output(git: &Path, dir: &Path, args: &[&str]) -> Result<Output, String> {
    let path = std::env::var_os("PATH").ok_or_else(|| "PATH is unset".to_owned())?;
    Command::new(git)
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", path)
        .env("HOME", dir)
        .env("GIT_CONFIG_GLOBAL", dir.join("empty-gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "1")
        .output()
        .map_err(|err| err.to_string())
}

fn git_success(git: &Path, dir: &Path, args: &[&str]) -> Result<Output, String> {
    let output = git_output(git, dir, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn git_fixture(dir: &Path, git: &Path) -> Result<PathBuf, String> {
    fs::write(dir.join("empty-gitconfig"), "").map_err(|err| err.to_string())?;
    git_success(git, dir, &["init", "--quiet"])?;
    git_success(git, dir, &["config", "user.name", "Velnor Test"])?;
    git_success(
        git,
        dir,
        &["config", "user.email", "velnor@example.invalid"],
    )?;
    let tracked = dir.join("tracked.txt");
    fs::write(&tracked, "same bytes\n").map_err(|err| err.to_string())?;
    git_success(git, dir, &["add", "tracked.txt"])?;
    git_success(git, dir, &["commit", "--quiet", "-m", "seed"])?;
    Ok(tracked)
}

fn index_bytes(dir: &Path) -> Result<Vec<u8>, String> {
    fs::read(dir.join(".git").join("index")).map_err(|err| err.to_string())
}

fn set_file_mtime_ahead(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let modified = SystemTime::now() + Duration::from_secs(5);
    file.set_modified(modified).map_err(|err| err.to_string())
}

fn last_env_value<'a>(env: &'a [(OsString, OsString)], key: &OsStr) -> Option<&'a OsStr> {
    env.iter()
        .rev()
        .find(|(found, _)| found == key)
        .map(|(_, value)| value.as_os_str())
}

fn run_discovery_status(dir: &Path) -> Result<(), String> {
    let discovery = IsolatedCommand::direct(
        "git",
        ["status", "--porcelain", "--untracked-files=no"]
            .into_iter()
            .map(OsString::from)
            .collect(),
    )
    .with_env(&[
        (OsString::from("HOME"), dir.as_os_str().to_owned()),
        (OsString::from("GIT_DIR"), dir.join(".git").into_os_string()),
        (OsString::from("GIT_WORK_TREE"), dir.as_os_str().to_owned()),
        (
            OsString::from("GIT_INDEX_FILE"),
            dir.join(".git").join("index").into_os_string(),
        ),
        (
            OsString::from("GIT_CONFIG_GLOBAL"),
            dir.join("empty-gitconfig").into_os_string(),
        ),
        (OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("1")),
        (OsString::from("GIT_TERMINAL_PROMPT"), OsString::from("0")),
    ])
    .map_err(|err| err.to_string())?
    .with_cwd(dir.to_path_buf());
    let output = discovery.run().map_err(|err| err.to_string())?;
    if output.success {
        Ok(())
    } else {
        Err(format!("discovery git status failed: {output:?}"))
    }
}

fn run_repo_task_git_add(git: &Path, dir: &Path) -> Result<(), String> {
    let declared = [
        (OsString::from("HOME"), dir.as_os_str().to_owned()),
        (
            OsString::from("GIT_CONFIG_GLOBAL"),
            dir.join("empty-gitconfig").into_os_string(),
        ),
        (OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("1")),
    ];
    let add = IsolatedCommand::repo_task(
        git.to_str()
            .ok_or_else(|| "git path is not UTF-8".to_owned())?,
        ["add", "tracked.txt"]
            .into_iter()
            .map(OsString::from)
            .collect(),
        &declared,
    )
    .map_err(|err| err.to_string())?
    .with_env(&[(OsString::from("GIT_OPTIONAL_LOCKS"), OsString::from("1"))])
    .map_err(|err| err.to_string())?
    .with_cwd(dir.to_path_buf());
    let output = add.run().map_err(|err| err.to_string())?;
    if output.success {
        Ok(())
    } else {
        Err(format!("explicit repo-task git add failed: {output:?}"))
    }
}

#[test]
fn allowlist_accepts_exactly_five_verbs() {
    assert_eq!(
        ALLOWED_GIT_VERBS,
        ["rev-parse", "ls-files", "diff", "show", "config"]
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
    let key = OsString::from("GIT_OPTIONAL_LOCKS");
    let one = OsString::from("1");
    for verb in ALLOWED_GIT_VERBS {
        let command = GitRequest::new(verb, Vec::new())
            .map_err(|err| err.to_string())?
            .command();
        assert_eq!(
            last_env_value(&command.full_env(), &key),
            Some(OsStr::new("0")),
            "discovery Git must disable optional locks for {verb}"
        );
        assert_eq!(
            last_env_value(&command.spawn_env(&[(key.clone(), one.clone())]), &key),
            Some(OsStr::new("0")),
            "discovery policy must override a hostile parent for {verb}"
        );
        assert!(matches!(
            command.with_env(&[(key.clone(), one.clone())]),
            Err(MiseError::InvalidStepInput { field, .. }) if field == "GIT_OPTIONAL_LOCKS"
        ));
    }

    let repo_task =
        IsolatedCommand::repo_task("git", Vec::new(), &[]).map_err(|err| err.to_string())?;
    assert_eq!(last_env_value(&repo_task.full_env(), &key), None);
    assert!(repo_task.with_env(&[(key, one)]).is_ok());
    Ok(())
}

#[test]
fn live_discovery_status_preserves_index_and_repo_task_add_remains_explicit() -> Result<(), String>
{
    let dir = scratch_dir("git-optional-locks")?;
    let git = git_executable()?;
    let tracked = git_fixture(&dir, &git)?;
    let index = index_bytes(&dir)?;
    set_file_mtime_ahead(&tracked)?;
    run_discovery_status(&dir)?;
    assert_eq!(
        index_bytes(&dir)?,
        index,
        "discovery status must not refresh the index"
    );

    git_success(
        &git,
        &dir,
        &["status", "--porcelain", "--untracked-files=no"],
    )?;
    let refreshed = index_bytes(&dir)?;
    assert_ne!(
        refreshed, index,
        "optional-lock control should refresh the index"
    );

    fs::write(&tracked, "changed bytes\n").map_err(|err| err.to_string())?;
    run_repo_task_git_add(&git, &dir)?;
    assert_ne!(
        index_bytes(&dir)?,
        refreshed,
        "repo-task git add must update the index"
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

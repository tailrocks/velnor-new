//! P10 `git config` cases: allowlist shape plus live origin reads in a
//! normal repo and a linked worktree through the typed `GitRequest`.
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use velnor_actions_mise::{ALLOWED_GIT_VERBS, GitRequest, is_allowed_git_verb};

/// P10 origin value shared by the repo and its linked worktree.
const ORIGIN: &str = "https://github.com/tailrocks/velnor-new.git";

/// Fresh scratch directory for one git fixture.
fn scratch_dir(test: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("velnor-mise-{test}-{}", std::process::id()));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) | Err(_) => {}
    }
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
}

/// Run git, failing the test on a nonzero status.
fn git(args: &[&str], cwd: &Path) -> Result<(), String> {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .map_err(|err| err.to_string())?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
}

/// Run git with owned arguments (for dynamic paths).
fn git_owned(args: &[String], cwd: &Path) -> Result<(), String> {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .map_err(|err| err.to_string())?;
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
    Ok(())
}

/// Minimal repo with identity and one commit.
fn make_repo(test: &str) -> Result<PathBuf, String> {
    let root = scratch_dir(test)?;
    git(&["init", "-b", "testmain"], &root)?;
    git(&["config", "user.email", "test@example.com"], &root)?;
    git(&["config", "user.name", "Test"], &root)?;
    git(&["config", "commit.gpgsign", "false"], &root)?;
    git(&["remote", "add", "origin", ORIGIN], &root)?;
    std::fs::write(root.join("file.txt"), "one\n").map_err(|err| err.to_string())?;
    git(&["add", "."], &root)?;
    git(&["commit", "-m", "one"], &root)?;
    Ok(root)
}

/// Typed `config --get remote.origin.url` request.
fn origin_request() -> GitRequest {
    GitRequest::config(vec![
        OsString::from("--get"),
        OsString::from("remote.origin.url"),
    ])
}

#[test]
fn config_verb_is_allowlisted() {
    assert!(
        ALLOWED_GIT_VERBS.contains(&"config"),
        "{ALLOWED_GIT_VERBS:?}"
    );
    assert!(is_allowed_git_verb("config"));
    assert!(GitRequest::new("config", Vec::new()).is_ok());
    let request = origin_request();
    assert_eq!(request.verb(), "config");
    assert_eq!(
        request.argv(),
        vec![
            OsString::from("git"),
            OsString::from("config"),
            OsString::from("--get"),
            OsString::from("remote.origin.url"),
        ]
    );
}

#[test]
fn config_get_reads_origin_through_worktree() -> Result<(), String> {
    let root = make_repo("p10-config-wt")?;
    let holder = scratch_dir("p10-config-wt-holder")?;
    let wt = holder.join("wt");
    git_owned(
        &[
            "worktree".to_owned(),
            "add".to_owned(),
            wt.display().to_string(),
            "-b".to_owned(),
            "p10cfg".to_owned(),
        ],
        &root,
    )?;
    for cwd in [&root, &wt] {
        let output = origin_request()
            .run_in(cwd)
            .map_err(|err| err.to_string())?;
        assert!(
            output.success,
            "config --get must succeed in {}",
            cwd.display()
        );
        let url = output.stdout_text("git").map_err(|err| err.to_string())?;
        assert_eq!(url.trim(), ORIGIN);
    }
    Ok(())
}

#[test]
fn config_get_missing_key_is_typed_failure() -> Result<(), String> {
    let root = make_repo("p10-config-missing")?;
    git(&["remote", "remove", "origin"], &root)?;
    let output = origin_request()
        .run_in(&root)
        .map_err(|err| err.to_string())?;
    assert!(
        !output.success,
        "missing key must fail without a spawn error"
    );
    assert_ne!(output.code, Some(0));
    Ok(())
}

#[test]
fn config_get_resolves_include() -> Result<(), String> {
    let root = make_repo("p10-config-include")?;
    git(&["remote", "remove", "origin"], &root)?;
    let inc = root.join("shared.inc");
    std::fs::write(
        &inc,
        "[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n",
    )
    .map_err(|err| err.to_string())?;
    git_owned(
        &[
            "config".to_owned(),
            "include.path".to_owned(),
            inc.display().to_string(),
        ],
        &root,
    )?;
    let output = origin_request()
        .run_in(&root)
        .map_err(|err| err.to_string())?;
    assert!(output.success, "include must resolve");
    let url = output.stdout_text("git").map_err(|err| err.to_string())?;
    assert_eq!(url.trim(), ORIGIN);
    Ok(())
}

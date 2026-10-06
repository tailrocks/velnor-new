//! Git verb allowlist cases, including live read-only executions.
use std::ffi::OsString;
use std::path::PathBuf;
use velnor_actions_mise::{ALLOWED_GIT_VERBS, GitRequest, MiseError, is_allowed_git_verb};

fn scratch_dir(test: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("velnor-mise-{test}-{}", std::process::id()));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) | Err(_) => {}
    }
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir)
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

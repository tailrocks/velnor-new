//! End-to-end CLI tests for `[github] unmanaged` and `velnor-actions unmanaged`.

use std::error::Error;
use std::fs;
use std::path::Path;

use crate::impl_cli_tmp::{
    cleanup, code, commit_all, fresh_tempdir, init_repo, spawn, write_workspace,
};

fn repo_with_github_table(prefix: &str, table: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let repo = fresh_tempdir(prefix)?;
    init_repo(&repo)?;
    write_workspace(&repo, 1)?;
    let config = repo.join(".velnor/config.toml");
    let mut text = fs::read_to_string(&config)?;
    text.push_str(table);
    fs::write(&config, text)?;
    commit_all(&repo)?;
    Ok(repo)
}

fn stray(repo: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(repo.join(".github"))?;
    fs::write(repo.join(".github/CODEOWNERS"), "* @owner\n")?;
    Ok(())
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn default_policy_preserves_stray_files_silently() -> Result<(), Box<dyn Error>> {
    let repo = repo_with_github_table("unmanaged-default", "")?;
    stray(&repo)?;
    let outer = fresh_tempdir("unmanaged-default-preview")?;
    let preview = outer.join("p");
    let preview = preview.to_str().ok_or("non-utf8 path")?;
    let generated = spawn(&["generate", "--output-dir", preview], &[], &repo)?;
    assert_eq!(code(&generated), 0, "{}", stderr(&generated));
    assert!(!stderr(&generated).contains("unmanaged_github"));
    let listed = spawn(&["unmanaged"], &[], &repo)?;
    assert_eq!(code(&listed), 0);
    assert_eq!(stdout(&listed), ".github/CODEOWNERS\n");
    cleanup(&repo);
    cleanup(&outer);
    Ok(())
}

#[test]
fn warn_prints_notice_and_succeeds() -> Result<(), Box<dyn Error>> {
    let repo = repo_with_github_table("unmanaged-warn", "\n[github]\nunmanaged = \"warn\"\n")?;
    stray(&repo)?;
    let generated = spawn(&["generate"], &[], &repo)?;
    assert_eq!(code(&generated), 0, "{}", stderr(&generated));
    assert!(
        stderr(&generated)
            .contains("velnor-actions: WARNING: unmanaged_github_entry:.github/CODEOWNERS")
    );
    assert!(repo.join(".github/CODEOWNERS").is_file());
    cleanup(&repo);
    Ok(())
}

#[test]
fn deny_fails_the_drift_generation_and_leaves_github_untouched() -> Result<(), Box<dyn Error>> {
    let repo = repo_with_github_table("unmanaged-deny", "\n[github]\nunmanaged = \"deny\"\n")?;
    stray(&repo)?;
    let outer = fresh_tempdir("unmanaged-deny-preview")?;
    let preview = outer.join("p");
    let preview_arg = preview.to_str().ok_or("non-utf8 path")?;
    let preview_run = spawn(&["generate", "--output-dir", preview_arg], &[], &repo)?;
    assert_ne!(code(&preview_run), 0);
    assert!(stderr(&preview_run).contains("unmanaged_github_entries:.github/CODEOWNERS"));
    assert!(!preview.join(".github").exists());
    let in_place = spawn(&["generate"], &[], &repo)?;
    assert_ne!(code(&in_place), 0);
    assert!(!repo.join(".github/workflows").exists());
    let listed = spawn(&["unmanaged"], &[], &repo)?;
    assert_eq!(code(&listed), 1);
    assert_eq!(stdout(&listed), ".github/CODEOWNERS\n");
    cleanup(&repo);
    cleanup(&outer);
    Ok(())
}

#[test]
fn deny_accepts_generated_output_and_allowlisted_paths() -> Result<(), Box<dyn Error>> {
    let table = "\n[github]\nunmanaged = \"deny\"\nallow = [\".github/CODEOWNERS\"]\n";
    let repo = repo_with_github_table("unmanaged-allow", table)?;
    stray(&repo)?;
    for _ in 0..2 {
        let generated = spawn(&["generate"], &[], &repo)?;
        assert_eq!(code(&generated), 0, "{}", stderr(&generated));
    }
    let listed = spawn(&["unmanaged"], &[], &repo)?;
    assert_eq!(code(&listed), 0);
    assert_eq!(stdout(&listed), "");
    cleanup(&repo);
    Ok(())
}

#[test]
fn invalid_allow_entry_is_a_config_error() -> Result<(), Box<dyn Error>> {
    let table = "\n[github]\nunmanaged = \"deny\"\nallow = [\"CODEOWNERS\"]\n";
    let repo = repo_with_github_table("unmanaged-bad-allow", table)?;
    let listed = spawn(&["unmanaged"], &[], &repo)?;
    assert_ne!(code(&listed), 0);
    assert!(stderr(&listed).contains("allow_outside_github"));
    cleanup(&repo);
    Ok(())
}

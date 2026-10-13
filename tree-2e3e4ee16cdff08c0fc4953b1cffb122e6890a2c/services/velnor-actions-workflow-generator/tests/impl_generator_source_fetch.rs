use super::impl_generator_source_scratch::Scratch;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use velnor_actions_workflow_generator::generator_release::QUALIFICATION_SOURCE_PREPARE;

#[test]
fn fetch_uses_exact_public_commit_without_credentials_or_helpers() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let workspace = scratch.0.join("workspace");
    let bin = scratch.0.join("bin");
    let log = scratch.0.join("git-arguments");
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&bin)?;
    let git = bin.join("git");
    fs::write(
        &git,
        r#"#!/bin/sh
set -eu
test -z "${GITHUB_TOKEN:-}"
test -z "${GH_TOKEN:-}"
test -z "${ACTIONS_RUNTIME_TOKEN:-}"
test -z "${ACTIONS_ID_TOKEN_REQUEST_TOKEN:-}"
test -z "${MISE_GITHUB_TOKEN:-}"
printf '%s\n' "$@" >> "$MOCK_GIT_LOG"
if [ "$1" = rev-parse ]; then printf '%s\n' "$MOCK_EXPECTED_SHA"; fi
"#,
    )?;
    let mut permissions = fs::metadata(&git)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&git, permissions)?;
    let original_path = std::env::var("PATH")?;
    let path = format!("{}:{original_path}", bin.display());
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let result = Command::new("bash")
        .arg("-c")
        .arg(QUALIFICATION_SOURCE_PREPARE)
        .env("PATH", path)
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_SHA", commit)
        .env("GITHUB_TOKEN", "workflow-token")
        .env("GH_TOKEN", "gh-token")
        .env("ACTIONS_RUNTIME_TOKEN", "runtime-token")
        .env("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "oidc-token")
        .env("MISE_GITHUB_TOKEN", "mise-token")
        .env("MOCK_GIT_LOG", &log)
        .env("MOCK_EXPECTED_SHA", commit)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let arguments = fs::read_to_string(log)?;
    assert!(arguments.contains("https://github.com/tailrocks/velnor-new.git"));
    assert!(arguments.contains(commit));
    assert!(arguments.contains("credential.helper="));
    assert!(arguments.contains("--depth=1"));
    assert!(arguments.contains("--no-tags"));
    assert!(QUALIFICATION_SOURCE_PREPARE.contains("timeout=60"));
    Ok(())
}

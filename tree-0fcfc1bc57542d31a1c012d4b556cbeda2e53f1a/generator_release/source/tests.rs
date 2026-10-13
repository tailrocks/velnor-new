use super::QUALIFICATION_SOURCE_PREPARE;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod fetch;
mod mise_setup;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-qualification-source-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[test]
fn reject_invalid_source_identity_before_running_git() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let workspace = scratch.0.join("workspace");
    fs::create_dir_all(&workspace)?;
    let log = scratch.0.join("git-arguments");
    let result = Command::new("bash")
        .arg("-c")
        .arg(QUALIFICATION_SOURCE_PREPARE)
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_REPOSITORY", "attacker/other")
        .env("GITHUB_SHA", "not-a-commit")
        .env("MOCK_GIT_LOG", &log)
        .output()?;
    assert!(!result.status.success());
    assert!(!log.exists(), "Git ran for an invalid source identity");
    Ok(())
}

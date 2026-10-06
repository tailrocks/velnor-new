//! Locked-resolution Git repository policy.

use crate::impl_cli_tmp::git_fixture;

use std::error::Error;

use crate::impl_repo_policy::{read, repo_root};

#[test]
fn lockfile_committed_and_locked_used() -> Result<(), Box<dyn Error>> {
    assert_ne!(read("Cargo.lock")?.trim(), "");
    let tracked = git_fixture::command(&repo_root())?
        .arg("ls-files")
        .arg("--error-unmatch")
        .arg("Cargo.lock")
        .current_dir(repo_root())
        .output()?;
    assert!(tracked.status.success(), "Cargo.lock not committed");
    for file in [
        "crates/velnor-actions-mise/src/requests.rs",
        "crates/velnor-actions-orchestrator/src/vectors.rs",
        ".github/workflows/ci.yml",
    ] {
        assert!(read(file)?.contains("--locked"), "{file} misses --locked");
    }
    Ok(())
}

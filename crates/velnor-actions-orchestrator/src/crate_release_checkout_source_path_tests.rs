// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn source_checkout_preserves_tracked_policy_path() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let source = root.join("remote-source");
    let owner = root.join("owner");
    let policy = root.join("policy");
    let runner = root.join("runner");
    for path in [&source, &owner, &policy, &runner] {
        fs::create_dir(path)?;
    }
    let (sha, _) = seed_remote(&root, &source, &owner)?;
    fs::write(source.join("release-source"), "tracked policy data\n")?;
    git_fixture(&source, &["add", "release-source"])?;
    git_fixture(&source, &["commit", "-qm", "tracked source name"])?;
    git_fixture(&source, &["push", "-q", "origin", "main"])?;
    let policy_sha = git_fixture(&source, &["rev-parse", "HEAD"])?;
    let (origin, stop, _, worker) = server(root.clone())?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let context = CheckoutContext {
            origin: &origin,
            test_http: true,
            deadline: velnor_actions_mise::CheckDeadline::after(Duration::from_secs(30))?,
        };
        checkout(
            &SystemGit,
            &policy,
            "owner/demo",
            &policy_sha,
            DUMMY,
            context,
        )?;
        let selected = create_source_directory(&policy, &runner)?;
        checkout(&SystemGit, &selected, "owner/demo", &sha, DUMMY, context)?;
        assert_eq!(git_fixture(&selected, &["rev-parse", "HEAD"])?, sha);
        assert_eq!(git_fixture(&policy, &["rev-parse", "HEAD"])?, policy_sha);
        assert_eq!(
            fs::read_to_string(policy.join("release-source"))?,
            "tracked policy data\n"
        );
        assert_eq!(git_fixture(&policy, &["status", "--porcelain"])?, "");
        assert!(!selected.starts_with(&policy));
        assert!(create_source_directory(&policy, &runner).is_err());
        Ok(())
    })();
    stop.store(true, Ordering::Relaxed);
    worker.join().map_err(|_| "server panicked")?;
    result
}

#[cfg(unix)]
#[test]
fn source_checkout_rejects_unsafe_temp_and_existing_paths() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let policy = root.join("policy");
    let runner = root.join("runner");
    fs::create_dir(&policy)?;
    fs::create_dir(&runner)?;
    assert!(source_directory(&policy, Path::new("relative")).is_err());
    assert!(source_directory(&policy, &policy).is_err());
    let linked_runner = root.join("linked-runner");
    symlink(&runner, &linked_runner)?;
    assert!(source_directory(&policy, &linked_runner).is_err());
    let destination = source_directory(&policy, &runner)?;
    fs::write(&destination, "existing data")?;
    assert!(create_source_directory(&policy, &runner).is_err());
    assert_eq!(fs::read_to_string(&destination)?, "existing data");
    fs::remove_file(&destination)?;
    symlink(&policy, &destination)?;
    assert!(source_directory(&policy, &runner).is_err());
    assert!(create_source_directory(&policy, &runner).is_err());
    assert!(policy.is_dir());
    Ok(())
}

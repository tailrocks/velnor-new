//! Baseline lookup repo-resolution tests.
//!
//! Declared via `#[path]` from `shard_baseline.rs` under `cfg(test)` so the
//! lookup module keeps the file size gate.

use super::*;

/// Minimal git checkout with `origin` set to `url`.
fn git_checkout_with_origin(url: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let git = tmp.path().join(".git");
    std::fs::create_dir_all(git.join("objects")).expect("objects");
    std::fs::create_dir_all(git.join("refs")).expect("refs");
    std::fs::write(git.join("HEAD"), "ref: refs/heads/testmain\n").expect("HEAD");
    std::fs::write(
        git.join("config"),
        format!("[remote \"origin\"]\n\turl = {url}\n"),
    )
    .expect("config");
    tmp
}

/// Lookup repo resolution follows the environment hermetically: no
/// spawn happens here, so the expectation derives from the same env
/// the resolver reads.
#[test]
fn lookup_repo_resolution_is_env_hermetic() {
    use crate::origin::validate_repository_slug;
    let env = std::env::var(crate::origin::GITHUB_REPOSITORY_ENV)
        .ok()
        .and_then(|raw| validate_repository_slug(&raw));
    // Bare checkout: env slug when set, else unresolved.
    let bare = tempfile::tempdir().expect("tempdir");
    match &env {
        Some(slug) => assert_eq!(resolve_lookup_repo(bare.path()), Ok(slug.clone())),
        None => assert_eq!(
            resolve_lookup_repo(bare.path()),
            Err("baseline_repo_unresolved".to_owned())
        ),
    }
    // Origin checkout: agreement resolves, disagreement conflicts.
    let origin = env.clone().unwrap_or_else(|| "o/r".to_owned());
    let checkout = git_checkout_with_origin(&format!("https://github.com/{origin}.git"));
    assert_eq!(resolve_lookup_repo(checkout.path()), Ok(origin));
    let other = if env.as_deref() == Some("o/r") {
        "evil/fork"
    } else {
        "o/r"
    };
    let forked = git_checkout_with_origin(&format!("https://github.com/{other}.git"));
    match &env {
        Some(_) => assert_eq!(
            resolve_lookup_repo(forked.path()),
            Err("baseline_repo_conflict".to_owned())
        ),
        None => assert_eq!(resolve_lookup_repo(forked.path()), Ok(other.to_owned())),
    }
}

/// `gh` stdout decodes under its explicit cap only: small listings
/// pass through, runaway output and undecodable bytes miss before
/// any JSON parsing.
#[test]
fn gh_stdout_cap_misses_before_parsing() {
    use velnor_actions_mise::ProcessOutput;
    let output = |stdout: Vec<u8>| ProcessOutput {
        stdout,
        stderr: Vec::new(),
        code: Some(0),
        signal: None,
        success: true,
    };
    assert_eq!(
        gh_stdout_checked(&output(b"[]".to_vec())),
        Ok("[]".to_owned())
    );
    let big = vec![b'x'; MAX_GH_STDOUT_BYTES + 1];
    assert_eq!(
        gh_stdout_checked(&output(big)),
        Err("baseline_unavailable".to_owned())
    );
    assert_eq!(
        gh_stdout_checked(&output(vec![0xff, 0xfe])),
        Err("baseline_unavailable".to_owned())
    );
}

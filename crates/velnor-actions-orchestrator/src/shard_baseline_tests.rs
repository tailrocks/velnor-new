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

/// Lookup repo resolution follows the explicit request slug: no
/// spawn happens here and no ambient env is read, so identical
/// inputs resolve identically under any runner environment.
#[test]
fn lookup_repo_resolution_is_request_hermetic() {
    // Bare checkout: request slug when set, else unresolved.
    let bare = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        resolve_lookup_repo(bare.path(), Some("o/r")),
        Ok("o/r".to_owned())
    );
    assert_eq!(
        resolve_lookup_repo(bare.path(), None),
        Err("baseline_repo_unresolved".to_owned())
    );
    // Origin checkout: agreement resolves, disagreement conflicts.
    let checkout = git_checkout_with_origin("https://github.com/o/r.git");
    assert_eq!(
        resolve_lookup_repo(checkout.path(), Some("o/r")),
        Ok("o/r".to_owned())
    );
    assert_eq!(
        resolve_lookup_repo(checkout.path(), None),
        Ok("o/r".to_owned())
    );
    let forked = git_checkout_with_origin("https://github.com/evil/fork.git");
    assert_eq!(
        resolve_lookup_repo(forked.path(), Some("o/r")),
        Err("baseline_repo_conflict".to_owned())
    );
    assert_eq!(
        resolve_lookup_repo(forked.path(), None),
        Ok("evil/fork".to_owned())
    );
}

/// Resolution misses before spawning without a known artifact name.
///
/// The repo, base, and workflow all validate, so only the missing
/// name can fail: no `gh` process ever starts on this path.
#[test]
fn resolve_manifests_requires_exact_artifact_before_spawn() {
    let catalog = ToolCatalog::pinned();
    let checkout = git_checkout_with_origin("https://github.com/o/r.git");
    let base = "a".repeat(40);
    for artifact in [None, Some("")] {
        assert_eq!(
            resolve_manifests(
                &catalog,
                checkout.path(),
                &base,
                ".github/workflows/ci.yml",
                "testmain",
                artifact,
                Some("o/r"),
            )
            .expect_err("must miss"),
            "baseline_no_exact_artifact".to_owned()
        );
    }
    assert_eq!(
        resolve_manifests(
            &catalog,
            checkout.path(),
            "short",
            ".github/workflows/ci.yml",
            "testmain",
            Some("velnor-baseline-x"),
            Some("o/r"),
        )
        .expect_err("must miss"),
        "base_must_be_full_sha".to_owned()
    );
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

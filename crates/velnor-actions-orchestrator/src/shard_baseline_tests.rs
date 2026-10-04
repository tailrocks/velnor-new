//! Exact baseline lookup scope and bounded service query regressions.

use super::*;
use velnor_actions_mise::{ProcessOutput, RuntimePaths};

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

#[test]
fn lookup_repo_resolution_is_request_hermetic() {
    let bare = tempfile::tempdir().expect("bare checkout");
    assert_eq!(
        resolve_lookup_repo(bare.path(), Some("o/r")),
        Ok("o/r".to_owned())
    );
    assert_eq!(
        resolve_lookup_repo(bare.path(), None),
        Err("baseline_repo_unresolved".to_owned())
    );
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

#[test]
fn invalid_compatibility_or_base_misses_before_spawn() {
    let catalog = ToolCatalog::pinned();
    let checkout = git_checkout_with_origin("https://github.com/o/r.git");
    let base = "a".repeat(40);
    for compatibility in ["", "not-a-digest"] {
        let artifact = format!("velnor-baseline-{base}-{compatibility}");
        assert_eq!(
            resolve_manifests(
                &catalog,
                checkout.path(),
                &base,
                ".github/workflows/ci.yml",
                "testmain",
                &artifact,
                Some("o/r"),
                RuntimePaths::full(),
            )
            .expect_err("must miss"),
            "baseline_no_exact_artifact"
        );
    }
    assert_eq!(
        resolve_manifests(
            &catalog,
            checkout.path(),
            "short",
            ".github/workflows/ci.yml",
            "testmain",
            "velnor-baseline-short-not-a-digest",
            Some("o/r"),
            RuntimePaths::full(),
        )
        .expect_err("must miss"),
        "base_must_be_full_sha"
    );
}

#[test]
fn gh_stdout_cap_misses_before_parsing() {
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

#[test]
fn lookup_artifact_and_attempt_queries_are_fixed_and_paginated() {
    let lookup = BaselineLookup::new(&"a".repeat(40), ".github/workflows/ci.yml", "main", "o/r")
        .expect("lookup");
    let strings = |args: Vec<std::ffi::OsString>| {
        args.iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        strings(lookup.artifacts_args(7)),
        [
            "api",
            "repos/o/r/actions/runs/7/artifacts",
            "--paginate",
            "--slurp",
        ]
    );
    assert_eq!(
        strings(lookup.attempt_args(7, 2)),
        ["api", "repos/o/r/actions/runs/7/attempts/2"]
    );
}

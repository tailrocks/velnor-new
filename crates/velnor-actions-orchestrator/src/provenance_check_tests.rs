//! Provenance validation tests (P04 matrix unit cases).
//!
//! Declared via `#[path]` from `provenance_check.rs` under `cfg(test)`.
//! Anchor cases run against real git fixtures through the shared
//! [`crate::origin::origin_url_via_git`] helper: nothing here hand-writes
//! `.git/config` and expects the anchor to follow.

use std::path::Path;
use std::process::Command as StdCommand;

use super::*;

/// Valid manifest plus matching expectations over `base`.
fn manifest_and_expected(base: &str) -> (BaselineManifest, ProvenanceExpectations) {
    let digest = digest_b3(b"d");
    let manifest = BaselineManifest {
        schema: 1,
        repository_id: digest.clone(),
        source_commit: base.to_owned(),
        ref_: "refs/heads/testmain".to_owned(),
        event: "push".to_owned(),
        workflow_ref: "o/r/.github/workflows/velnor.yml@refs/heads/testmain".to_owned(),
        run_id: 7,
        run_attempt: 1,
        final_status: "passed".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        compatibility_id: digest.clone(),
        artifact_id: 9,
        artifact_name: baseline_artifact_name(base, &digest).expect("name"),
        tasks: Vec::new(),
        expires_at_unix: None,
    };
    let expected = ProvenanceExpectations {
        base: base.to_owned(),
        branch: "testmain".to_owned(),
        workflow_path: ".github/workflows/velnor.yml".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        repository_id: Some(digest),
    };
    (manifest, expected)
}

/// Run git in `cwd`, failing the test on a nonzero exit.
fn git(args: &[&str], cwd: &Path) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
}

/// Run git with owned arguments (for dynamic paths).
fn git_owned(args: &[String], cwd: &Path) {
    let status = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
}

/// Fresh git fixture with identity configured and signing disabled.
fn make_git_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git(&["init", "-b", "testmain"], root);
    git(&["config", "user.email", "test@example.com"], root);
    git(&["config", "user.name", "Test"], root);
    git(&["config", "commit.gpgsign", "false"], root);
    tmp
}

/// Expected anchor for the `o/r` fixture origin.
fn fixture_anchor() -> String {
    digest_b3("github.com/o/r".as_bytes())
}

#[test]
fn every_wrong_dimension_fails_validation() {
    let base = "a".repeat(40);
    let (manifest, expected) = manifest_and_expected(&base);
    assert!(validate_provenance(&manifest, &digest_b3(b"m"), &expected).is_ok());
    let check = |mutate: &dyn Fn(&mut BaselineManifest), reason: &str| {
        let (mut manifest, expected) = manifest_and_expected(&base);
        mutate(&mut manifest);
        assert_eq!(
            validate_provenance(&manifest, &digest_b3(b"m"), &expected).expect_err(reason),
            reason.to_owned()
        );
    };
    check(&|m| m.schema = 2, "stale_schema:migration_required:v2");
    check(&|m| m.schema = 0, "stale_schema:migration_required:v0");
    check(&|m| m.source_commit = "b".repeat(40), "wrong_commit");
    check(&|m| m.ref_ = "refs/heads/other".to_owned(), "wrong_ref");
    check(&|m| m.event = "pull_request".to_owned(), "untrusted_proof");
    check(&|m| m.run_id = 0, "bad_proof_identity");
    check(
        &|m| m.generator_version = "9.9.9".to_owned(),
        "generator_mismatch",
    );
    check(
        &|m| m.generator_sha256 = "0".repeat(64),
        "generator_unverifiable",
    );
    check(
        &|m| m.workflow_ref = "o/r/other.yml@refs/heads/testmain".to_owned(),
        "wrong_workflow",
    );
    check(
        &|m| m.artifact_name = "forged".to_owned(),
        "artifact_mismatch",
    );
}

#[test]
fn repository_anchor_and_origins_normalize() {
    assert_eq!(
        normalize_origin_url("https://github.com/O/R.git"),
        Some("github.com/o/r".to_owned())
    );
    assert_eq!(
        normalize_origin_url("git@github.com:O/R.git"),
        Some("github.com/o/r".to_owned())
    );
    assert!(normalize_origin_url("not-a-url").is_none());
    let repo = make_git_repo();
    let root = repo.path();
    git(
        &["remote", "add", "origin", "https://github.com/o/r.git"],
        root,
    );
    let anchor = repository_anchor_from_origin(root).expect("anchor");
    assert_eq!(anchor, fixture_anchor());
    git(
        &["remote", "set-url", "origin", "git@github.com:O/R.git"],
        root,
    );
    assert_eq!(repository_anchor_from_origin(root).expect("scp"), anchor);
    let base = "a".repeat(40);
    let (mut manifest, mut expected) = manifest_and_expected(&base);
    expected.repository_id = Some(anchor.clone());
    manifest.repository_id = anchor;
    assert!(validate_provenance(&manifest, &digest_b3(b"m"), &expected).is_ok());
    manifest.repository_id = digest_b3(b"other");
    assert_eq!(
        validate_provenance(&manifest, &digest_b3(b"m"), &expected).expect_err("repo"),
        "wrong_repository".to_owned()
    );
    expected.repository_id = None;
    assert_eq!(
        validate_provenance(&manifest, &digest_b3(b"m"), &expected).expect_err("unanchored"),
        "repository_unanchored".to_owned()
    );
}

#[test]
fn linked_worktree_anchor_shares_remote_identity() {
    let repo = make_git_repo();
    let root = repo.path();
    git(
        &["remote", "add", "origin", "https://github.com/o/r.git"],
        root,
    );
    std::fs::write(root.join("f.txt"), "x\n").expect("fixture");
    git(&["add", "."], root);
    git(&["commit", "-m", "one"], root);
    let holder = tempfile::tempdir().expect("holder");
    let wt = holder.path().join("wt");
    git_owned(
        &[
            "worktree".to_owned(),
            "add".to_owned(),
            wt.display().to_string(),
            "-b".to_owned(),
            "p10anchor".to_owned(),
        ],
        root,
    );
    assert!(wt.join(".git").is_file(), "linked worktree has a .git file");
    let main = repository_anchor_from_origin(root).expect("main anchor");
    assert_eq!(main, fixture_anchor());
    assert_eq!(repository_anchor_from_origin(&wt).expect("wt anchor"), main);
}

#[test]
fn include_defined_origin_resolves() {
    let repo = make_git_repo();
    let root = repo.path();
    let inc = root.join("shared.inc");
    std::fs::write(
        &inc,
        "[remote \"origin\"]\n\turl = https://github.com/o/r.git\n",
    )
    .expect("include");
    git_owned(
        &[
            "config".to_owned(),
            "include.path".to_owned(),
            inc.display().to_string(),
        ],
        root,
    );
    let local = std::fs::read_to_string(root.join(".git/config")).expect("config");
    assert!(
        !local.contains("[remote \"origin\"]"),
        "origin must live only in the include"
    );
    assert_eq!(
        repository_anchor_from_origin(root).expect("anchor"),
        fixture_anchor()
    );
}

#[test]
fn missing_and_mismatched_origins_fail_closed() {
    let plain = tempfile::tempdir().expect("tempdir");
    assert!(repository_anchor_from_origin(plain.path()).is_none());
    let repo = make_git_repo();
    let root = repo.path();
    assert!(repository_anchor_from_origin(root).is_none());
    git(
        &["remote", "add", "origin", "https://github.com/o/r.git"],
        root,
    );
    let anchor = repository_anchor_from_origin(root).expect("anchor");
    git(
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/evil/other.git",
        ],
        root,
    );
    let other = repository_anchor_from_origin(root).expect("other anchor");
    assert_ne!(anchor, other);
    git(&["remote", "set-url", "origin", "not-a-url"], root);
    assert!(repository_anchor_from_origin(root).is_none());
}

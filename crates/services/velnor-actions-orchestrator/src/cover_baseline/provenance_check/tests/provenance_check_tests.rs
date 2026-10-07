//! Provenance validation tests (P04 matrix unit cases).
//!
//! Declared via `#[path]` from `provenance_check.rs` under `cfg(test)`.
//! Anchor cases build minimal git fixtures on disk and resolve them
//! through the real `git config` query in
//! [`velnor_actions_orchestrator_core::origin::origin_url_via_git`]: nothing here spawns git
//! (orchestrator sources never spawn), and nothing hand-reads the
//! fixture config back.

use super::*;

use std::path::Path;

/// Valid manifest plus matching expectations over `base`.
///
/// The repository id anchors `github.com/o/r` and the workflow slug
/// names `o/r`, so slug, anchor, and manifest binding all agree.
fn manifest_and_expected(base: &str) -> (BaselineManifest, ProvenanceExpectations) {
    let digest = digest_b3(b"d");
    let anchor = digest_b3("github.com/o/r".as_bytes());
    let name = baseline_artifact_name(base, &digest).expect("name");
    let manifest = BaselineManifest {
        schema: 2,
        repository_id: anchor.clone(),
        source_commit: base.to_owned(),
        ref_: "refs/heads/testmain".to_owned(),
        event: "push".to_owned(),
        workflow_ref: "o/r/.github/workflows/ci.yml@refs/heads/testmain".to_owned(),
        run_id: 7,
        run_attempt: 1,
        final_status: "passed".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        compatibility_id: digest.clone(),
        artifact_id: crate::cover_compat::baseline_artifact_numeric_id(&name),
        artifact_name: name,
        tasks: Vec::new(),
        expires_at_unix: None,
    };
    let expected = ProvenanceExpectations {
        base: base.to_owned(),
        branch: "testmain".to_owned(),
        workflow_path: ".github/workflows/ci.yml".to_owned(),
        generator_version: "0.1.0".to_owned(),
        generator_sha256: "1".repeat(64),
        repository_id: Some(anchor),
        repository_slug: Some("o/r".to_owned()),
        repository_conflict: false,
    };
    (manifest, expected)
}

/// Minimal git dir: HEAD, config, and the object/ref scaffolding real
/// `git config` needs for repository discovery.
fn write_git_dir(root: &Path, config: &str) {
    let git = root.join(".git");
    std::fs::create_dir_all(git.join("objects")).expect("objects");
    std::fs::create_dir_all(git.join("refs")).expect("refs");
    std::fs::write(git.join("HEAD"), "ref: refs/heads/testmain\n").expect("HEAD");
    std::fs::write(git.join("config"), config).expect("config");
}

/// Fresh git fixture whose origin is `url`, when given.
fn make_git_repo(url: Option<&str>) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = match url {
        Some(url) => {
            format!("[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = {url}\n")
        }
        None => "[core]\n\trepositoryformatversion = 0\n".to_owned(),
    };
    write_git_dir(tmp.path(), &config);
    tmp
}

/// Point an existing fixture's origin at `url`.
fn set_origin(root: &Path, url: &str) {
    let config =
        format!("[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = {url}\n");
    std::fs::write(root.join(".git/config"), config).expect("config");
}

/// Link `wt` to the fixture at `main` the way `git worktree add` does:
/// a `.git` file plus the administrative dir under the common git dir.
fn link_worktree(main: &Path, wt: &Path) {
    let admin = main.join(".git/worktrees/wt");
    std::fs::create_dir_all(&admin).expect("worktrees");
    std::fs::create_dir_all(wt).expect("wt");
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).expect("wt git");
    std::fs::write(
        admin.join("gitdir"),
        format!("{}\n", wt.join(".git").display()),
    )
    .expect("gitdir");
    std::fs::write(admin.join("commondir"), "../..\n").expect("commondir");
    std::fs::write(admin.join("HEAD"), "ref: refs/heads/testmain\n").expect("wt HEAD");
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
    check(&|m| m.schema = 1, "stale_schema:migration_required:v1");
    check(&|m| m.schema = 0, "stale_schema:migration_required:v0");
    check(&|m| m.source_commit = "b".repeat(40), "wrong_commit");
    check(&|m| m.source_commit = "A".repeat(40), "bad_source_commit");
    check(&|m| m.ref_ = "refs/heads/other".to_owned(), "wrong_ref");
    check(&|m| m.event = "pull_request".to_owned(), "untrusted_proof");
    check(&|m| m.event = "merge_group".to_owned(), "untrusted_proof");
    check(&|m| m.event = "fork".to_owned(), "untrusted_proof");
    check(&|m| m.final_status = "failed".to_owned(), "untrusted_proof");
    check(&|m| m.run_id = 0, "bad_proof_identity");
    check(&|m| m.run_attempt = 0, "bad_proof_identity");
    check(&|m| m.artifact_id = 0, "bad_proof_identity");
    check(
        &|m| m.generator_version = "9.9.9".to_owned(),
        "generator_mismatch",
    );
    check(
        &|m| m.generator_sha256 = "f".repeat(64),
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
        &|m| m.workflow_ref = "evil/fork/.github/workflows/ci.yml@refs/heads/testmain".to_owned(),
        "wrong_repository",
    );
    check(
        &|m| m.workflow_ref = "o/r/.github/workflows/ci.yml@refs/heads/other".to_owned(),
        "wrong_workflow",
    );
    check(
        &|m| m.workflow_ref = "not-a-ref".to_owned(),
        "bad_workflow_ref",
    );
    check(
        &|m| m.repository_id = digest_b3(b"evil"),
        "wrong_repository",
    );
    check(
        &|m| m.artifact_name = "forged".to_owned(),
        "artifact_mismatch",
    );
    check(
        &|m| {
            // A service-assigned ID the manifest author read back: well
            // over zero, but not the name fingerprint, so it fails.
            m.artifact_id = m.artifact_id.wrapping_add(1).max(1);
        },
        "artifact_mismatch",
    );
}

#[test]
fn repository_slug_and_origins_normalize() {
    assert_eq!(
        normalize_origin_url("https://github.com/O/R.git"),
        Some("github.com/o/r".to_owned())
    );
    assert_eq!(
        normalize_origin_url("git@github.com:O/R.git"),
        Some("github.com/o/r".to_owned())
    );
    assert!(normalize_origin_url("not-a-url").is_none());
    let repo = make_git_repo(Some("https://github.com/o/r.git"));
    let root = repo.path();
    let slug = repository_slug_from_origin(root).expect("slug");
    assert_eq!(slug, "o/r");
    assert_eq!(repository_anchor_for_slug(&slug), fixture_anchor());
    set_origin(root, "git@github.com:O/R.git");
    assert_eq!(repository_slug_from_origin(root).as_deref(), Some("o/r"));
    let base = "a".repeat(40);
    let (mut manifest, mut expected) = manifest_and_expected(&base);
    expected.repository_id = Some(fixture_anchor());
    manifest.repository_id = fixture_anchor();
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
    let repo = make_git_repo(Some("https://github.com/o/r.git"));
    let root = repo.path();
    let holder = tempfile::tempdir().expect("holder");
    let wt = holder.path().join("wt");
    link_worktree(root, &wt);
    assert!(wt.join(".git").is_file(), "linked worktree has a .git file");
    let main = repository_slug_from_origin(root).expect("main slug");
    assert_eq!(main, "o/r");
    assert_eq!(repository_anchor_for_slug(&main), fixture_anchor());
    assert_eq!(repository_slug_from_origin(&wt).expect("wt slug"), main);
}

#[test]
fn include_defined_origin_resolves() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let inc = root.join("shared.inc");
    std::fs::write(
        &inc,
        "[remote \"origin\"]\n\turl = https://github.com/o/r.git\n",
    )
    .expect("include");
    write_git_dir(root, &format!("[include]\n\tpath = {}\n", inc.display()));
    let local = std::fs::read_to_string(root.join(".git/config")).expect("config");
    assert!(
        !local.contains("[remote \"origin\"]"),
        "origin must live only in the include"
    );
    let slug = repository_slug_from_origin(root).expect("slug");
    assert_eq!(slug, "o/r");
    assert_eq!(repository_anchor_for_slug(&slug), fixture_anchor());
}

/// Valid task entry over `digest`, observed by the manifest run.
fn task_entry(digest: &str) -> crate::merge::required_evidence::BaselineTaskEntry {
    crate::merge::required_evidence::BaselineTaskEntry {
        task_id: "stack/rust/root/clippy/default".to_owned(),
        task_digest: digest.to_owned(),
        input_digest: digest.to_owned(),
        closure_digest: digest.to_owned(),
        proof_run_id: 7,
        observed_run_id: 7,
        external_data: None,
        proof: None,
    }
}

/// Structured proof binding `entry`, with `input_digest` overridden.
fn task_proof(
    entry: &crate::merge::required_evidence::BaselineTaskEntry,
    input_digest: &str,
) -> velnor_actions_contract_workflow::ManifestTaskProof {
    velnor_actions_contract_workflow::ManifestTaskProof::new(
        &entry.task_id,
        &entry.task_digest,
        input_digest,
        &entry.task_digest,
        &entry.task_digest,
        &entry.task_digest,
        &entry.task_digest,
        "default",
        entry.proof_run_id,
    )
    .expect("proof")
}

/// Stale external-data freshness with a malformed identity digest.
fn stale_external_data() -> crate::external_data::ExternalDataFreshness {
    crate::external_data::ExternalDataFreshness {
        source: "advisory-db".to_owned(),
        identity: "bogus".to_owned(),
        age_secs: 60,
    }
}

#[test]
fn task_entries_validate_identity_runs_freshness_and_proof_binding() {
    let base = "a".repeat(40);
    let digest = digest_b3(b"d");
    let run = |entry: &crate::merge::required_evidence::BaselineTaskEntry| {
        let (mut manifest, expected) = manifest_and_expected(&base);
        manifest.tasks = vec![entry.clone()];
        validate_provenance(&manifest, &digest_b3(b"m"), &expected)
    };
    let valid = task_entry(&digest);
    assert!(run(&valid).is_ok());
    let proven = crate::merge::required_evidence::BaselineTaskEntry {
        proof: Some(task_proof(&valid, &valid.input_digest)),
        ..valid.clone()
    };
    assert!(run(&proven).is_ok());
    let check = |mutate: &dyn Fn(&mut crate::merge::required_evidence::BaselineTaskEntry),
                 reason: &str| {
        let mut entry = valid.clone();
        mutate(&mut entry);
        assert_eq!(run(&entry).expect_err(reason), reason.to_owned());
    };
    check(
        &|entry| entry.closure_digest = "bogus".to_owned(),
        "bad_task_identity",
    );
    check(
        &|entry| entry.task_id = "bogus".to_owned(),
        "bad_task_identity",
    );
    check(&|entry| entry.proof_run_id = 0, "bad_proof_identity");
    check(&|entry| entry.observed_run_id = 0, "bad_proof_identity");
    check(&|entry| entry.observed_run_id = 8, "proof_mismatch");
    check(
        &|entry| entry.external_data = Some(stale_external_data()),
        "bad_external_data",
    );
    let mut mismatched = valid.clone();
    mismatched.proof = Some(task_proof(&valid, &digest_b3(b"forged")));
    assert_eq!(
        run(&mismatched).expect_err("proof"),
        "proof_mismatch".to_owned()
    );
    // A structured proof adds its binding check on top: identity, run,
    // and freshness checks still run instead of being skipped.
    let mut unobserved = valid.clone();
    unobserved.proof = Some(task_proof(&valid, &valid.input_digest));
    unobserved.observed_run_id = 0;
    assert_eq!(
        run(&unobserved).expect_err("runs"),
        "bad_proof_identity".to_owned()
    );
    let mut stale = valid;
    stale.proof = Some(task_proof(&stale.clone(), &stale.input_digest));
    stale.external_data = Some(stale_external_data());
    assert_eq!(
        run(&stale).expect_err("freshness"),
        "bad_external_data".to_owned()
    );
}

#[test]
fn missing_and_mismatched_origins_fail_closed() {
    let plain = tempfile::tempdir().expect("tempdir");
    assert!(repository_slug_from_origin(plain.path()).is_none());
    let repo = make_git_repo(None);
    let root = repo.path();
    assert!(repository_slug_from_origin(root).is_none());
    set_origin(root, "https://github.com/o/r.git");
    let slug = repository_slug_from_origin(root).expect("slug");
    assert_eq!(slug, "o/r");
    set_origin(root, "https://github.com/evil/other.git");
    assert_eq!(
        repository_slug_from_origin(root).as_deref(),
        Some("evil/other")
    );
    assert_ne!(
        repository_anchor_for_slug(&slug),
        repository_anchor_for_slug("evil/other")
    );
    set_origin(root, "https://ghe.example.com/o/r.git");
    assert!(
        repository_slug_from_origin(root).is_none(),
        "non-github origins have no comparable slug"
    );
    set_origin(root, "not-a-url");
    assert!(repository_slug_from_origin(root).is_none());
}

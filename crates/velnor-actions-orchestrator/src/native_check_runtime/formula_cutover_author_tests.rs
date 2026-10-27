// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path, time::Duration};

use super::admit_cutover_author_pull;
use crate::{
    git_fixture,
    github_landed_pull::GitHubRestUser,
    homebrew::structural_cutover::{protected_transition, verify_author_policy},
    internal_plan::native_homebrew_checks::{
        NativeCheckAuthorityIdentity, NativeCheckIdentityExtension,
    },
    package_release_publish::PublicPullRequestSnapshot,
};
use tempfile::TempDir;
use velnor_actions_contract::{
    WorkflowEvent,
    config::{CheckPlatform, NativeHomebrewOperation},
    package_release::PackageReleaseChannel,
};
use velnor_actions_mise::CheckDeadline;

const REPOSITORY: &str = "jackin-project/homebrew-tap";
const REPOSITORY_ID: u64 = 1_200_197_694;
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

struct GitFixture {
    directory: TempDir,
    base: String,
    head: String,
    next_head: String,
    merge: String,
    queue: String,
    unrelated: String,
    other_base: String,
}

impl GitFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        git(directory.path(), &["init", "--quiet"])?;
        let base = commit(directory.path(), &[], "base")?;
        let head = commit(directory.path(), &[&base], "head")?;
        let next_head = commit(directory.path(), &[&base], "next head")?;
        let merge = commit(directory.path(), &[&base, &head], "merge")?;
        let queue = commit(directory.path(), &[&base], "synthetic merge group")?;
        let unrelated = commit(directory.path(), &[], "unrelated")?;
        let other_base = commit(directory.path(), &[], "other base")?;
        git(
            directory.path(),
            &["symbolic-ref", "HEAD", "refs/heads/main"],
        )?;
        set_head(&directory, &head)?;
        Ok(Self {
            directory,
            base,
            head,
            next_head,
            merge,
            queue,
            unrelated,
            other_base,
        })
    }

    fn checkout(&self, sha: &str) -> Result<(), Box<dyn Error>> {
        set_head(&self.directory, sha)
    }

    fn root(&self) -> &Path {
        self.directory.path()
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let mut command = if root.join(".git").exists() {
        git_fixture::command(root)?
    } else {
        let mut command = git_fixture::sterile_command();
        command.current_dir(root);
        command
    };
    let output = command
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn commit(root: &Path, parents: &[&str], message: &str) -> Result<String, Box<dyn Error>> {
    let mut args = vec![
        "-c",
        "user.name=Cutover Test",
        "-c",
        "user.email=cutover@example.invalid",
        "commit-tree",
        EMPTY_TREE,
    ];
    for parent in parents {
        args.extend(["-p", parent]);
    }
    args.extend(["-m", message]);
    git(root, &args)
}

fn set_head(directory: &TempDir, sha: &str) -> Result<(), Box<dyn Error>> {
    git(directory.path(), &["update-ref", "refs/heads/main", sha])?;
    Ok(())
}

fn expected_author() -> GitHubRestUser {
    let transition = protected_transition().expect("protected transition");
    verify_author_policy(transition.transition()).expect("compiled author policy")
}

fn snapshot(
    head_sha: &str,
    base_sha: &str,
    merge_ref_sha: Option<&str>,
    author: GitHubRestUser,
) -> PublicPullRequestSnapshot {
    PublicPullRequestSnapshot {
        state: "open".to_owned(),
        draft: false,
        merged: false,
        base_repository: REPOSITORY.to_owned(),
        base_repository_id: REPOSITORY_ID,
        base_ref: "main".to_owned(),
        base_sha: base_sha.to_owned(),
        head_repository: REPOSITORY.to_owned(),
        head_repository_id: REPOSITORY_ID,
        head_ref: "cutover".to_owned(),
        head_sha: head_sha.to_owned(),
        merge_ref_sha: merge_ref_sha.map(str::to_owned),
        author: Some(author),
    }
}

fn admit(
    fixture: &GitFixture,
    pull: &PublicPullRequestSnapshot,
    checkout_head: &str,
) -> Result<(), crate::OrchestratorError> {
    let transition = protected_transition().expect("protected transition");
    admit_cutover_author_pull(
        fixture.root(),
        pull,
        transition.transition(),
        &fixture.base,
        checkout_head,
        &expected_author(),
        CheckDeadline::after(Duration::from_secs(10)).expect("deadline"),
    )
}

fn checkout_proof(
    fixture: &GitFixture,
    pull: &PublicPullRequestSnapshot,
    expected_base: &str,
    checkout_head: &str,
) -> Result<(), crate::OrchestratorError> {
    crate::homebrew::authority::verify_homebrew_authority_pull_request_checkout(
        fixture.root(),
        pull,
        expected_base,
        &pull.head_sha,
        checkout_head,
        CheckDeadline::after(Duration::from_secs(10)).expect("deadline"),
    )
}

#[test]
fn structural_cutover_author_admission_accepts_exact_head_or_merge_and_rejects_identity_drift()
-> Result<(), Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    fixture.checkout(&fixture.head)?;
    let direct = snapshot(&fixture.head, &fixture.base, None, expected_author());
    let direct_proof = checkout_proof(&fixture, &direct, &fixture.base, &fixture.head);
    let direct_result = admit(&fixture, &direct, &fixture.head);

    fixture.checkout(&fixture.merge)?;
    let merge = snapshot(
        &fixture.head,
        &fixture.base,
        Some(&fixture.merge),
        expected_author(),
    );
    let merge_proof = checkout_proof(&fixture, &merge, &fixture.base, &fixture.merge);
    let merge_result = admit(&fixture, &merge, &fixture.merge);

    fixture.checkout(&fixture.unrelated)?;
    let unrelated = snapshot(&fixture.head, &fixture.base, None, expected_author());
    let unrelated_proof = checkout_proof(&fixture, &unrelated, &fixture.base, &fixture.unrelated);
    let unrelated_result = admit(&fixture, &unrelated, &fixture.unrelated);

    fixture.checkout(&fixture.merge)?;
    let stale_merge = snapshot(
        &fixture.next_head,
        &fixture.base,
        Some(&fixture.merge),
        expected_author(),
    );
    let stale_merge_proof = checkout_proof(&fixture, &stale_merge, &fixture.base, &fixture.merge);
    let stale_merge_result = admit(&fixture, &stale_merge, &fixture.merge);

    fixture.checkout(&fixture.merge)?;
    let changed_base = snapshot(
        &fixture.head,
        &fixture.other_base,
        Some(&fixture.merge),
        expected_author(),
    );
    let changed_base_proof =
        checkout_proof(&fixture, &changed_base, &fixture.other_base, &fixture.merge);
    let changed_base_result = admit(&fixture, &changed_base, &fixture.merge);

    fixture.checkout(&fixture.head)?;
    let wrong_author = snapshot(
        &fixture.head,
        &fixture.base,
        None,
        GitHubRestUser {
            id: 139_018,
            login: "donbeave".to_owned(),
            account_type: "User".to_owned(),
        },
    );
    let wrong_author_proof = checkout_proof(&fixture, &wrong_author, &fixture.base, &fixture.head);
    let wrong_author_result = admit(&fixture, &wrong_author, &fixture.head);

    assert!(
        direct_proof.is_ok(),
        "direct Git parent proof: {direct_proof:?}"
    );
    assert!(direct_result.is_ok(), "direct head: {direct_result:?}");
    assert!(
        merge_proof.is_ok(),
        "exact Git merge parent proof: {merge_proof:?}"
    );
    assert!(merge_result.is_ok(), "exact merge: {merge_result:?}");
    assert!(unrelated_proof.is_err(), "unrelated Git checkout");
    assert!(unrelated_result.is_err(), "unrelated checkout");
    assert!(stale_merge_proof.is_err(), "stale merge Git parentage");
    assert!(stale_merge_result.is_err(), "stale merge parentage");
    assert!(changed_base_proof.is_err(), "changed base Git parentage");
    assert!(changed_base_result.is_err(), "changed base");
    assert!(
        wrong_author_proof.is_ok(),
        "author does not alter Git parent proof"
    );
    assert!(wrong_author_result.is_err(), "wrong author");
    Ok(())
}

fn queue_identity(fixture: &GitFixture) -> NativeCheckIdentityExtension {
    NativeCheckIdentityExtension {
        schema: "homebrew-native-check-identity-v2".to_owned(),
        check_id: "cutover".to_owned(),
        operation: NativeHomebrewOperation::FormulaContract,
        formula_channel: Some(PackageReleaseChannel::Preview),
        operation_version: "native-homebrew-check-v1".to_owned(),
        sdk_version: "0.1.57".to_owned(),
        sdk_sha256: "0".repeat(64),
        authority: NativeCheckAuthorityIdentity::Seed {
            transition_id: "cutover".to_owned(),
            transition_digest: "0".repeat(64),
            base_tree: EMPTY_TREE.to_owned(),
            sdk_source_sha: fixture.head.clone(),
            sdk_version: "0.1.57".to_owned(),
            sdk_manifest_sha256: "0".repeat(64),
            sdk_seed_runner_binary_sha256: "0".repeat(64),
        },
        tap_repository: REPOSITORY.to_owned(),
        tap_repository_id: REPOSITORY_ID,
        tap_base_sha: fixture.base.clone(),
        head: fixture.queue.clone(),
        tree_object: EMPTY_TREE.to_owned(),
        formula_path: None,
        formula_blob_sha: None,
        runner: "ubuntu-latest".to_owned(),
        platform: CheckPlatform::LinuxX64,
        tool_profile: "formula_contract-linux-x86_64".to_owned(),
    }
}

#[path = "formula_cutover_author_merge_group_tests.rs"]
mod merge_group_tests;

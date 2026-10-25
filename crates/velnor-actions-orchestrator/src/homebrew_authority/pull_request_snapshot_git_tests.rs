use crate::git_fixture;
use serde_json::json;
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::CheckDeadline;

use super::{RunnerEnvironment, admit_snapshot, parse_runner, same_selection, verify_checkout};

const REPOSITORY: &str = "owner/tap";
const REPOSITORY_ID: u64 = 7;

struct Commits {
    base: String,
    head: String,
    merge: String,
    fabricated: String,
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = git_fixture::command(root)
        .expect("fixture repository authority")
        .args(args)
        .output()
        .expect("run Git fixture command");
    assert!(
        output.status.success(),
        "Git fixture command failed: {args:?}"
    );
    String::from_utf8(output.stdout)
        .expect("Git fixture stdout")
        .trim()
        .to_owned()
}

fn commit(root: &std::path::Path, message: &str) {
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-m",
            message,
            "--no-gpg-sign",
        ],
    );
}

fn commits(root: &std::path::Path) -> Commits {
    let status = git_fixture::sterile_command()
        .current_dir(root)
        .args(["init", "-q"])
        .status()
        .expect("initialize isolated Git fixture");
    assert!(status.success(), "Git fixture initialization failed");
    std::fs::write(root.join("formula.rb"), "base\n").expect("base file");
    git(root, &["add", "formula.rb"]);
    commit(root, "base");
    let base = git(root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("formula.rb"), "head\n").expect("head file");
    git(root, &["add", "formula.rb"]);
    commit(root, "head");
    let head = git(root, &["rev-parse", "HEAD"]);
    let head_tree = git(root, &["write-tree"]);
    let merge = git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit-tree",
            &head_tree,
            "-p",
            &base,
            "-p",
            &head,
            "-m",
            "API merge",
        ],
    );
    std::fs::write(root.join("formula.rb"), "fabricated\n").expect("fabricated file");
    git(root, &["add", "formula.rb"]);
    let fabricated_tree = git(root, &["write-tree"]);
    let fabricated = git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit-tree",
            &fabricated_tree,
            "-p",
            &base,
            "-p",
            &head,
            "-m",
            "not API merge",
        ],
    );
    Commits {
        base,
        head,
        merge,
        fabricated,
    }
}

fn checkout(root: &std::path::Path, sha: &str) {
    git(root, &["checkout", "--detach", sha]);
}

fn pull_snapshot(commits: &Commits) -> crate::package_release_publish::PublicPullRequestSnapshot {
    crate::package_release_publish::PublicPullRequestSnapshot {
        state: "open".to_owned(),
        draft: false,
        merged: false,
        base_repository: REPOSITORY.to_owned(),
        base_repository_id: REPOSITORY_ID,
        base_ref: "main".to_owned(),
        base_sha: commits.base.clone(),
        head_repository: REPOSITORY.to_owned(),
        head_repository_id: REPOSITORY_ID,
        head_ref: "topic".to_owned(),
        head_sha: commits.head.clone(),
        merge_commit_sha: Some(commits.merge.clone()),
    }
}

fn runner(commits: &Commits) -> super::RunnerFacts {
    let payload = json!({
        "repository": {"id": REPOSITORY_ID, "full_name": REPOSITORY},
        "pull_request": {
            "number": 12,
            "base": {
                "sha": commits.base,
                "ref": "main",
                "repo": {"id": REPOSITORY_ID, "full_name": REPOSITORY}
            },
            "head": {
                "sha": commits.head,
                "ref": "topic",
                "repo": {"id": REPOSITORY_ID, "full_name": REPOSITORY, "fork": false}
            }
        }
    });
    parse_runner(
        WorkflowEvent::PullRequest,
        12,
        REPOSITORY,
        RunnerEnvironment {
            event_name: "pull_request".to_owned(),
            repository: REPOSITORY.to_owned(),
            sha: commits.merge.clone(),
            git_ref: "refs/pull/12/merge".to_owned(),
        },
        &payload,
    )
    .expect("captured PR facts")
}

fn admitted_identity(
    root: &std::path::Path,
    pull: &crate::package_release_publish::PublicPullRequestSnapshot,
    commits: &Commits,
) -> Result<(String, String), crate::OrchestratorError> {
    let head = git(root, &["rev-parse", "HEAD"]);
    let facts = runner(commits);
    let identity = admit_snapshot(
        pull,
        WorkflowEvent::PullRequest,
        REPOSITORY,
        REPOSITORY_ID,
        &commits.head,
        &head,
        &facts,
    )?;
    verify_checkout(
        root,
        pull,
        &identity.0,
        &identity.1,
        &head,
        CheckDeadline::after(std::time::Duration::from_secs(5)).expect("deadline"),
    )?;
    Ok(identity)
}

#[test]
fn captured_pr_snapshot_accepts_head_or_exact_api_merge_and_rejects_fabricated_tree() {
    let temporary = tempfile::tempdir().expect("repository root");
    let physical_root = temporary
        .path()
        .canonicalize()
        .expect("physical repository root");
    let root = physical_root.as_path();
    let commits = commits(root);
    let pull = pull_snapshot(&commits);

    checkout(root, &commits.head);
    let direct = admitted_identity(root, &pull, &commits).expect("direct PR head checkout");
    assert_eq!(direct, (commits.base.clone(), commits.head.clone()));

    checkout(root, &commits.merge);
    let merge = admitted_identity(root, &pull, &commits).expect("API merge checkout");
    assert_eq!(merge, (commits.base.clone(), commits.head.clone()));

    checkout(root, &commits.fabricated);
    assert!(admitted_identity(root, &pull, &commits).is_err());
}

#[test]
fn changed_api_base_head_or_merge_facts_cannot_revalidate_captured_context() {
    let temporary = tempfile::tempdir().expect("repository root");
    let physical_root = temporary
        .path()
        .canonicalize()
        .expect("physical repository root");
    let root = physical_root.as_path();
    let commits = commits(root);
    checkout(root, &commits.merge);
    let mut pull = pull_snapshot(&commits);
    let original = admitted_identity(root, &pull, &commits).expect("initial context");
    assert_eq!(original, (commits.base.clone(), commits.head.clone()));

    pull.base_sha = "d".repeat(40);
    let changed_base = admit_snapshot(
        &pull,
        WorkflowEvent::PullRequest,
        REPOSITORY,
        REPOSITORY_ID,
        &commits.head,
        &commits.merge,
        &runner(&commits),
    );
    assert!(changed_base.is_err());

    pull = pull_snapshot(&commits);
    pull.head_sha = "e".repeat(40);
    assert!(
        admit_snapshot(
            &pull,
            WorkflowEvent::PullRequest,
            REPOSITORY,
            REPOSITORY_ID,
            &commits.head,
            &commits.merge,
            &runner(&commits),
        )
        .is_err()
    );

    pull = pull_snapshot(&commits);
    pull.merge_commit_sha = Some("f".repeat(40));
    assert!(admitted_identity(root, &pull, &commits).is_err());
    let selected = captured_selection(&commits);
    assert!(same_selection(
        &selected,
        &commits.base,
        &commits.head,
        Some(&commits.merge),
    ));
    assert!(!same_selection(
        &selected,
        &"d".repeat(40),
        &commits.head,
        Some(&commits.merge),
    ));
    assert!(!same_selection(
        &selected,
        &commits.base,
        &"e".repeat(40),
        Some(&commits.merge),
    ));
    assert!(!same_selection(
        &selected,
        &commits.base,
        &commits.head,
        Some(&"f".repeat(40)),
    ));
}

fn captured_selection(commits: &Commits) -> super::PullRequestSelection {
    super::PullRequestSelection {
        number: 12,
        event: WorkflowEvent::PullRequest,
        base_sha: commits.base.clone(),
        head_sha: commits.head.clone(),
        merge_commit_sha: Some(commits.merge.clone()),
        checkout_head: commits.merge.clone(),
        repository: REPOSITORY.to_owned(),
        runner_sha: String::new(),
        runner_ref: String::new(),
        payload_base_sha: String::new(),
        payload_head_sha: String::new(),
        payload_head_repository: String::new(),
        payload_head_repository_id: REPOSITORY_ID,
        payload_base_repository: String::new(),
        payload_base_repository_id: REPOSITORY_ID,
        payload_base_ref: String::new(),
        payload_head_ref: String::new(),
        payload_repository_id: REPOSITORY_ID,
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct Server {
    state: RefCell<State>,
}

#[derive(Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Independent external failure scenarios"
)]
struct State {
    branch: Option<String>,
    commit: Option<Commit>,
    pulls: Vec<PullRequest>,
    lost_ref_reply: bool,
    lost_pr_reply: bool,
    bad_blob: bool,
    moved_base: bool,
}

impl WriterIo for Server {
    fn require_no_other_release_pr(&self, _: &Plan) -> Result<(), OrchestratorError> {
        Ok(())
    }
    fn require_not_release_pr(&self, _: &Plan) -> Result<(), OrchestratorError> {
        Ok(())
    }
    fn base_sha(&self, plan: &Plan) -> Result<String, OrchestratorError> {
        Ok(if self.state.borrow().moved_base {
            "f".repeat(40)
        } else {
            plan.source_sha.clone()
        })
    }
    fn branch_sha(&self, _: &Plan) -> Result<Option<String>, OrchestratorError> {
        Ok(self.state.borrow().branch.clone())
    }
    fn commit(&self, _: &Plan, _: &str) -> Result<Commit, OrchestratorError> {
        self.state
            .borrow()
            .commit
            .clone()
            .ok_or_else(|| internal("missing_commit"))
    }
    fn pull_requests(&self, _: &Plan) -> Result<Vec<PullRequest>, OrchestratorError> {
        Ok(self.state.borrow().pulls.clone())
    }
    fn create_blob(&self, _: &Plan, bytes: &[u8]) -> Result<String, OrchestratorError> {
        if self.state.borrow().bad_blob {
            Ok("f".repeat(40))
        } else {
            crate::release_policy_admission::git_blob_sha1(bytes)
        }
    }
    fn create_tree(
        &self,
        plan: &Plan,
        changes: &BTreeMap<String, TreeEntry>,
    ) -> Result<String, OrchestratorError> {
        let _ = (plan, changes);
        Ok("c".repeat(40))
    }
    fn create_commit(&self, plan: &Plan, tree: &str) -> Result<String, OrchestratorError> {
        let email = format!(
            "{}+github-actions[bot]@users.noreply.github.com",
            plan.author_id
        );
        let commit = Commit {
            sha: "d".repeat(40),
            tree: tree.to_owned(),
            parent: plan.source_sha.clone(),
            message: plan.message.clone(),
            author_name: "github-actions[bot]".into(),
            author_email: email.clone(),
            committer_name: "github-actions[bot]".into(),
            committer_email: email,
        };
        self.state.borrow_mut().commit = Some(commit.clone());
        Ok(commit.sha)
    }
    fn create_branch(&self, _: &Plan, sha: &str) -> Result<(), OrchestratorError> {
        let mut state = self.state.borrow_mut();
        state.branch = Some(sha.to_owned());
        if state.lost_ref_reply {
            Err(internal("lost_response"))
        } else {
            Ok(())
        }
    }
    fn create_pull_request(&self, plan: &Plan, sha: &str) -> Result<(), OrchestratorError> {
        let mut state = self.state.borrow_mut();
        state.pulls.push(PullRequest {
            number: 17,
            repository: plan.repository.clone(),
            head_repository: plan.repository.clone(),
            head: plan.branch.clone(),
            head_sha: sha.to_owned(),
            base: plan.base.clone(),
            author_id: plan.author_id,
            state: "open".into(),
            title: plan.title.clone(),
            body: plan.body.clone(),
        });
        if state.lost_pr_reply {
            Err(internal("lost_response"))
        } else {
            Ok(())
        }
    }
}

fn plan() -> Plan {
    Plan {
        repository: "owner/project".into(),
        base: "main".into(),
        source_sha: "a".repeat(40),
        base_tree: "b".repeat(40),
        expected_tree: "c".repeat(40),
        branch: "release-plz-123-1".into(),
        message: "chore: release".into(),
        title: "chore: release".into(),
        body: "Review release changes".into(),
        author_id: 42,
        allowed_paths: BTreeSet::from(["Cargo.toml".into()]),
        changes: BTreeMap::from([(
            "Cargo.toml".into(),
            ("100644".into(), b"new content".to_vec()),
        )]),
    }
}

#[test]
fn creates_and_reconciles_exact_branch_and_pull_request_after_lost_replies()
-> Result<(), OrchestratorError> {
    let server = Server::default();
    server.state.borrow_mut().lost_ref_reply = true;
    server.state.borrow_mut().lost_pr_reply = true;
    let plan = plan();
    assert_eq!(create_or_reconcile(&server, &plan)?, 17);
    assert_eq!(create_or_reconcile(&server, &plan)?, 17);
    assert_eq!(server.state.borrow().pulls.len(), 1);
    assert_eq!(
        server.state.borrow().branch.as_deref(),
        Some("dddddddddddddddddddddddddddddddddddddddd")
    );
    Ok(())
}

#[path = "crate_release_pr_writer_http_tests.rs"]
mod http_tests;

#[test]
fn conflicting_branch_and_untrusted_blob_never_create_pull_requests() {
    let plan = plan();
    let conflicting = Server::default();
    {
        let mut state = conflicting.state.borrow_mut();
        state.branch = Some("d".repeat(40));
        let email = format!(
            "{}+github-actions[bot]@users.noreply.github.com",
            plan.author_id
        );
        state.commit = Some(Commit {
            sha: "d".repeat(40),
            tree: "c".repeat(40),
            parent: "f".repeat(40),
            message: plan.message.clone(),
            author_name: "github-actions[bot]".into(),
            author_email: email.clone(),
            committer_name: "github-actions[bot]".into(),
            committer_email: email,
        });
    }
    assert!(create_or_reconcile(&conflicting, &plan).is_err());
    assert_eq!(conflicting.state.borrow().pulls.len(), 0);
    let corrupt_blob = Server::default();
    corrupt_blob.state.borrow_mut().bad_blob = true;
    assert!(create_or_reconcile(&corrupt_blob, &plan).is_err());
    assert!(corrupt_blob.state.borrow().branch.is_none());
    assert_eq!(corrupt_blob.state.borrow().pulls.len(), 0);
    let moved = Server::default();
    moved.state.borrow_mut().moved_base = true;
    assert!(create_or_reconcile(&moved, &plan).is_err());
    assert!(moved.state.borrow().branch.is_none());
}

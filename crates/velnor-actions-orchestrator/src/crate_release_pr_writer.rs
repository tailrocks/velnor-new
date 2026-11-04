// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::{OrchestratorError, internal::internal};

#[derive(Clone, Debug, PartialEq, Eq)]
struct TreeEntry {
    mode: String,
    sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Commit {
    sha: String,
    tree: String,
    parent: String,
    message: String,
    author_name: String,
    author_email: String,
    committer_name: String,
    committer_email: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PullRequest {
    number: u64,
    repository: String,
    head_repository: String,
    head: String,
    head_sha: String,
    base: String,
    author_id: u64,
    state: String,
    title: String,
    body: String,
}

struct Plan {
    repository: String,
    base: String,
    source_sha: String,
    base_tree: String,
    expected_tree: String,
    branch: String,
    message: String,
    title: String,
    body: String,
    author_id: u64,
    allowed_paths: BTreeSet<String>,
    changes: BTreeMap<String, (String, Vec<u8>)>,
}

pub(crate) fn write_prepared_pr(
    github: &crate::crate_release_github::NativeGitHubAdmissionRead,
    preparation: &crate::crate_release_pr_acquisition::VerifiedPreparation,
) -> Result<(), OrchestratorError> {
    let trees = crate::crate_release_pr_tree::GitHubTrees {
        api: github,
        repository: &preparation.intent.repository,
    };
    let source = &preparation.artifact.binding.source_sha;
    let plan = build_plan(
        &trees,
        &preparation.intent,
        source,
        &preparation.source_tree,
        &preparation.artifact,
    )?;
    create_or_reconcile(&NativeWriter { github }, &plan)?;
    Ok(())
}

fn build_plan<R: crate::crate_release_pr_tree::TreeRead>(
    trees: &R,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    source_sha: &str,
    source_tree_sha: &str,
    artifact: &crate::crate_release_pr_artifact::PreparationArtifact,
) -> Result<Plan, OrchestratorError> {
    let invalid = || internal("release_pr_plan_invalid");
    if artifact.binding.repository != intent.repository
        || artifact.binding.source_sha != source_sha
        || artifact.binding.workflow_sha != source_sha
        || artifact.binding.run_attempt != 1
        || artifact.binding.run_id == 0
    {
        return Err(invalid());
    }
    let allowed_paths: BTreeSet<_> = intent.pr_write_paths.iter().cloned().collect();
    let mut changes = BTreeMap::new();
    let mut overlay = BTreeMap::new();
    for file in &artifact.files {
        if !allowed_paths.contains(&file.path) {
            return Err(invalid());
        }
        let mode =
            crate::crate_release_pr_tree::regular_file_mode(trees, source_tree_sha, &file.path)?;
        let sha = crate::release_policy_admission::git_blob_sha1(&file.bytes)?;
        if changes
            .insert(file.path.clone(), (mode.clone(), file.bytes.clone()))
            .is_some()
        {
            return Err(invalid());
        }
        overlay.insert(
            file.path.clone(),
            crate::crate_release_pr_tree::Change { mode, sha },
        );
    }
    let expected_tree =
        crate::crate_release_pr_tree::overlay_tree(trees, source_tree_sha, &overlay)?;
    let plan = Plan {
        repository: intent.repository.clone(),
        base: intent.default_branch.clone(),
        source_sha: source_sha.to_owned(),
        base_tree: source_tree_sha.to_owned(),
        expected_tree,
        branch: format!("release-plz-{}-{}", artifact.binding.run_id, artifact.binding.run_attempt),
        message: "chore: release".to_owned(),
        title: "chore: release".to_owned(),
        body: "Automated release preparation. Review the version, dependency and changelog changes before merging.".to_owned(),
        author_id: 41_898_282,
        allowed_paths,
        changes,
    };
    validate_plan(&plan)?;
    Ok(plan)
}

trait WriterIo {
    fn require_not_release_pr(&self, plan: &Plan) -> Result<(), OrchestratorError>;
    fn require_no_other_release_pr(&self, plan: &Plan) -> Result<(), OrchestratorError>;
    fn base_sha(&self, plan: &Plan) -> Result<String, OrchestratorError>;
    fn branch_sha(&self, plan: &Plan) -> Result<Option<String>, OrchestratorError>;
    fn commit(&self, plan: &Plan, sha: &str) -> Result<Commit, OrchestratorError>;
    fn pull_requests(&self, plan: &Plan) -> Result<Vec<PullRequest>, OrchestratorError>;
    fn create_blob(&self, plan: &Plan, bytes: &[u8]) -> Result<String, OrchestratorError>;
    fn create_tree(
        &self,
        plan: &Plan,
        changes: &BTreeMap<String, TreeEntry>,
    ) -> Result<String, OrchestratorError>;
    fn create_commit(&self, plan: &Plan, tree: &str) -> Result<String, OrchestratorError>;
    fn create_branch(&self, plan: &Plan, sha: &str) -> Result<(), OrchestratorError>;
    fn create_pull_request(&self, plan: &Plan, sha: &str) -> Result<(), OrchestratorError>;
}

fn create_or_reconcile<I: WriterIo>(io: &I, plan: &Plan) -> Result<u64, OrchestratorError> {
    validate_plan(plan)?;
    require_base(io, plan)?;
    let expected = expected_tree(plan)?;
    let existing = io.branch_sha(plan)?;
    let head = if let Some(sha) = existing {
        verify_commit(io, plan, &sha)?
    } else {
        if !io.pull_requests(plan)?.is_empty() {
            return Err(internal("release_pr_conflict"));
        }
        create_branch(io, plan, &expected)?
    };
    require_base(io, plan)?;
    require_branch(io, plan, &head)?;
    if let Some(pr) = exact_pull_request(io, plan, &head)? {
        return Ok(pr);
    }
    require_base(io, plan)?;
    require_branch(io, plan, &head)?;
    io.require_not_release_pr(plan)?;
    io.require_no_other_release_pr(plan)?;
    drop(io.create_pull_request(plan, &head));
    let pr = exact_pull_request(io, plan, &head)?
        .ok_or_else(|| internal("release_pr_create_unreconciled"))?;
    require_base(io, plan)?;
    require_branch(io, plan, &head)?;
    Ok(pr)
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_plan(plan: &Plan) -> Result<(), OrchestratorError> {
    let Some((owner, repository)) = plan.repository.split_once('/') else {
        return Err(internal("release_pr_plan_invalid"));
    };
    if !velnor_actions_contract::config::is_valid_github_owner(owner)
        || !velnor_actions_contract::config::is_valid_github_repo_name(repository)
        || !velnor_actions_contract::is_valid_branch_name(&plan.base)
        || !velnor_actions_contract::is_valid_branch_name(&plan.branch)
        || !plan.branch.starts_with("release-plz-")
        || !valid_sha(&plan.source_sha)
        || !valid_sha(&plan.base_tree)
        || !valid_sha(&plan.expected_tree)
        || plan.base_tree == plan.expected_tree
        || plan.author_id == 0
        || plan.message.is_empty()
        || plan.title.is_empty()
        || plan.body.is_empty()
        || plan
            .changes
            .keys()
            .any(|path| !plan.allowed_paths.contains(path))
    {
        return Err(internal("release_pr_plan_invalid"));
    }
    Ok(())
}

fn require_branch<I: WriterIo>(
    io: &I,
    plan: &Plan,
    expected: &str,
) -> Result<(), OrchestratorError> {
    if io.branch_sha(plan)?.as_deref() != Some(expected) {
        return Err(internal("release_pr_branch_moved"));
    }
    Ok(())
}

fn require_base<I: WriterIo>(io: &I, plan: &Plan) -> Result<(), OrchestratorError> {
    if io.base_sha(plan)? != plan.source_sha {
        return Err(internal("release_pr_base_moved"));
    }
    Ok(())
}

fn expected_tree(plan: &Plan) -> Result<BTreeMap<String, TreeEntry>, OrchestratorError> {
    let mut expected = BTreeMap::new();
    if plan.changes.is_empty() {
        return Err(internal("release_pr_empty"));
    }
    for (path, (mode, bytes)) in &plan.changes {
        if !matches!(mode.as_str(), "100644" | "100755") {
            return Err(internal("release_pr_file_mode"));
        }
        let entry = TreeEntry {
            mode: mode.clone(),
            sha: crate::release_policy_admission::git_blob_sha1(bytes)?,
        };
        expected.insert(path.clone(), entry);
    }
    Ok(expected)
}

fn create_branch<I: WriterIo>(
    io: &I,
    plan: &Plan,
    expected: &BTreeMap<String, TreeEntry>,
) -> Result<String, OrchestratorError> {
    let mut changes = BTreeMap::new();
    for (path, (mode, bytes)) in &plan.changes {
        require_base(io, plan)?;
        let sha = io.create_blob(plan, bytes)?;
        if crate::release_policy_admission::git_blob_sha1(bytes)? != sha {
            return Err(internal("release_pr_blob_mismatch"));
        }
        changes.insert(
            path.clone(),
            TreeEntry {
                mode: mode.clone(),
                sha,
            },
        );
    }
    require_base(io, plan)?;
    let tree = io.create_tree(plan, &changes)?;
    if !valid_sha(&tree) || tree != plan.expected_tree || &changes != expected {
        return Err(internal("release_pr_tree_mismatch"));
    }
    require_base(io, plan)?;
    let sha = io.create_commit(plan, &tree)?;
    verify_commit(io, plan, &sha)?;
    require_base(io, plan)?;
    io.require_not_release_pr(plan)?;
    io.require_no_other_release_pr(plan)?;
    if io.branch_sha(plan)?.is_some() || !io.pull_requests(plan)?.is_empty() {
        return Err(internal("release_pr_conflict"));
    }
    require_base(io, plan)?;
    drop(io.create_branch(plan, &sha));
    let actual = io
        .branch_sha(plan)?
        .ok_or_else(|| internal("release_pr_branch_unreconciled"))?;
    verify_commit(io, plan, &actual)
}

fn verify_commit<I: WriterIo>(io: &I, plan: &Plan, sha: &str) -> Result<String, OrchestratorError> {
    if !valid_sha(sha) {
        return Err(internal("release_pr_branch_conflict"));
    }
    let commit = io.commit(plan, sha)?;
    let email = format!(
        "{}+github-actions[bot]@users.noreply.github.com",
        plan.author_id
    );
    if commit.sha != sha
        || commit.parent != plan.source_sha
        || commit.message != plan.message
        || commit.tree != plan.expected_tree
        || commit.author_name != "github-actions[bot]"
        || commit.committer_name != "github-actions[bot]"
        || commit.author_email != email
        || commit.committer_email != email
    {
        return Err(internal("release_pr_branch_conflict"));
    }
    Ok(sha.to_owned())
}

fn exact_pull_request<I: WriterIo>(
    io: &I,
    plan: &Plan,
    sha: &str,
) -> Result<Option<u64>, OrchestratorError> {
    let pulls = io.pull_requests(plan)?;
    if pulls.len() > 1 {
        return Err(internal("release_pr_duplicate"));
    }
    let Some(pr) = pulls.first() else {
        return Ok(None);
    };
    if pr.number == 0
        || pr.repository != plan.repository
        || pr.head_repository != plan.repository
        || pr.head != plan.branch
        || pr.head_sha != sha
        || pr.base != plan.base
        || pr.author_id != plan.author_id
        || pr.state != "open"
        || pr.title != plan.title
        || pr.body != plan.body
    {
        return Err(internal("release_pr_conflict"));
    }
    Ok(Some(pr.number))
}

struct NativeWriter<'a> {
    github: &'a crate::crate_release_github::NativeGitHubAdmissionRead,
}

impl NativeWriter<'_> {
    fn get(&self, route: &str) -> Result<serde_json::Value, OrchestratorError> {
        crate::crate_release_github::GitHubAdmissionRead::get(self.github, route)
    }

    fn post(
        &self,
        route: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, OrchestratorError> {
        self.github.reader.post(route, body)
    }
}

fn text(value: &serde_json::Value, key: &str) -> Result<String, OrchestratorError> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| internal("release_pr_response_invalid"))
}

impl WriterIo for NativeWriter<'_> {
    fn require_no_other_release_pr(&self, plan: &Plan) -> Result<(), OrchestratorError> {
        let base = crate::crate_release_http::encode(&plan.base);
        for page in 1..=100 {
            let value = self.get(&format!(
                "repos/{}/pulls?state=open&base={base}&per_page=100&page={page}",
                plan.repository
            ))?;
            let prs = value
                .as_array()
                .filter(|rows| rows.len() <= 100)
                .ok_or_else(|| internal("release_pr_response_invalid"))?;
            for pr in prs {
                let head = text(&pr["head"], "ref")?;
                let head_repo = text(&pr["head"]["repo"], "full_name")?;
                if head.starts_with("release-plz-")
                    && (head_repo != plan.repository || head != plan.branch)
                {
                    return Err(internal("release_pr_existing"));
                }
            }
            if prs.len() < 100 {
                return Ok(());
            }
        }
        Err(internal("release_pr_list_limit"))
    }

    fn require_not_release_pr(&self, plan: &Plan) -> Result<(), OrchestratorError> {
        if crate::crate_release_github::has_release_pr(
            self.github,
            &format!("repos/{}", plan.repository),
            &plan.source_sha,
        )? {
            return Err(internal("release_pr_source_is_release"));
        }
        Ok(())
    }

    fn base_sha(&self, plan: &Plan) -> Result<String, OrchestratorError> {
        let branch = crate::crate_release_http::encode(&plan.base);
        let value = self.get(&format!("repos/{}/git/ref/heads/{branch}", plan.repository))?;
        if value["ref"].as_str() != Some(format!("refs/heads/{}", plan.base).as_str())
            || value["object"]["type"].as_str() != Some("commit")
        {
            return Err(internal("release_pr_response_invalid"));
        }
        text(&value["object"], "sha")
    }

    fn branch_sha(&self, plan: &Plan) -> Result<Option<String>, OrchestratorError> {
        let branch = crate::crate_release_http::encode(&plan.branch);
        let value = self.get(&format!(
            "repos/{}/git/matching-refs/heads/{branch}",
            plan.repository
        ))?;
        let values = value
            .as_array()
            .filter(|items| items.len() < 1000)
            .ok_or_else(|| internal("release_pr_response_invalid"))?;
        let expected = format!("refs/heads/{}", plan.branch);
        let matches: Vec<_> = values
            .iter()
            .filter(|item| item["ref"].as_str() == Some(expected.as_str()))
            .collect();
        match matches.as_slice() {
            [] => Ok(None),
            [value] if value["object"]["type"].as_str() == Some("commit") => {
                text(&value["object"], "sha").map(Some)
            }
            _ => Err(internal("release_pr_response_invalid")),
        }
    }

    fn commit(&self, plan: &Plan, sha: &str) -> Result<Commit, OrchestratorError> {
        if !valid_sha(sha) {
            return Err(internal("release_pr_response_invalid"));
        }
        let value = self.get(&format!("repos/{}/git/commits/{sha}", plan.repository))?;
        let parents = value["parents"]
            .as_array()
            .filter(|items| items.len() == 1)
            .ok_or_else(|| internal("release_pr_response_invalid"))?;
        Ok(Commit {
            sha: text(&value, "sha")?,
            tree: text(&value["tree"], "sha")?,
            parent: text(&parents[0], "sha")?,
            message: text(&value, "message")?,
            author_name: text(&value["author"], "name")?,
            author_email: text(&value["author"], "email")?,
            committer_name: text(&value["committer"], "name")?,
            committer_email: text(&value["committer"], "email")?,
        })
    }

    fn pull_requests(&self, plan: &Plan) -> Result<Vec<PullRequest>, OrchestratorError> {
        let owner = plan
            .repository
            .split_once('/')
            .ok_or_else(|| internal("release_pr_plan_invalid"))?
            .0;
        let head = crate::crate_release_http::encode(&format!("{owner}:{}", plan.branch));
        let base = crate::crate_release_http::encode(&plan.base);
        let value = self.get(&format!(
            "repos/{}/pulls?state=all&head={head}&base={base}&per_page=100",
            plan.repository
        ))?;
        let values = value
            .as_array()
            .filter(|items| items.len() < 100)
            .ok_or_else(|| internal("release_pr_response_invalid"))?;
        values
            .iter()
            .map(|pr| {
                Ok(PullRequest {
                    number: pr["number"]
                        .as_u64()
                        .ok_or_else(|| internal("release_pr_response_invalid"))?,
                    repository: text(&pr["base"]["repo"], "full_name")?,
                    head_repository: text(&pr["head"]["repo"], "full_name")?,
                    head: text(&pr["head"], "ref")?,
                    head_sha: text(&pr["head"], "sha")?,
                    base: text(&pr["base"], "ref")?,
                    author_id: pr["user"]["id"]
                        .as_u64()
                        .ok_or_else(|| internal("release_pr_response_invalid"))?,
                    state: text(pr, "state")?,
                    title: text(pr, "title")?,
                    body: text(pr, "body")?,
                })
            })
            .collect()
    }

    fn create_blob(&self, plan: &Plan, bytes: &[u8]) -> Result<String, OrchestratorError> {
        use base64::Engine as _;
        let value = self.post(&format!("repos/{}/git/blobs", plan.repository), &serde_json::json!({"content":base64::engine::general_purpose::STANDARD.encode(bytes),"encoding":"base64"}))?;
        text(&value, "sha")
    }

    fn create_tree(
        &self,
        plan: &Plan,
        changes: &BTreeMap<String, TreeEntry>,
    ) -> Result<String, OrchestratorError> {
        let entries: Vec<_> = changes.iter().map(|(path, entry)| serde_json::json!({"path":path,"mode":entry.mode,"type":"blob","sha":entry.sha})).collect();
        let value = self.post(
            &format!("repos/{}/git/trees", plan.repository),
            &serde_json::json!({"base_tree":plan.base_tree,"tree":entries}),
        )?;
        text(&value, "sha")
    }

    fn create_commit(&self, plan: &Plan, tree: &str) -> Result<String, OrchestratorError> {
        let email = format!(
            "{}+github-actions[bot]@users.noreply.github.com",
            plan.author_id
        );
        let identity = serde_json::json!({"name":"github-actions[bot]","email":email});
        let value = self.post(&format!("repos/{}/git/commits", plan.repository), &serde_json::json!({"message":plan.message,"tree":tree,"parents":[plan.source_sha],"author":identity,"committer":identity}))?;
        text(&value, "sha")
    }

    fn create_branch(&self, plan: &Plan, sha: &str) -> Result<(), OrchestratorError> {
        self.post(
            &format!("repos/{}/git/refs", plan.repository),
            &serde_json::json!({"ref":format!("refs/heads/{}",plan.branch),"sha":sha}),
        )?;
        Ok(())
    }

    fn create_pull_request(&self, plan: &Plan, _: &str) -> Result<(), OrchestratorError> {
        self.post(&format!("repos/{}/pulls", plan.repository), &serde_json::json!({"title":plan.title,"body":plan.body,"head":plan.branch,"base":plan.base,"draft":false}))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
    #[expect(
        clippy::too_many_lines,
        reason = "The complete verified input is visible in one fixture"
    )]
    fn derives_reviewable_plan_from_verified_source_tree() -> Result<(), OrchestratorError> {
        use crate::crate_release_pr_tree::{GitHubTrees, TreeItem, git_tree_sha};
        struct Api {
            sha: String,
            blob: String,
        }
        impl crate::crate_release_github::GitHubAdmissionRead for Api {
            fn get(&self, route: &str) -> Result<serde_json::Value, OrchestratorError> {
                assert_eq!(route, format!("repos/owner/project/git/trees/{}", self.sha));
                Ok(
                    serde_json::json!({"sha":self.sha,"truncated":false,"tree":[{"path":"Cargo.toml","mode":"100755","type":"blob","sha":self.blob}]}),
                )
            }
            fn get_optional(
                &self,
                _: &str,
            ) -> Result<Option<serde_json::Value>, OrchestratorError> {
                Err(internal("unexpected_optional"))
            }
        }
        let old_blob = crate::release_policy_admission::git_blob_sha1(b"old")?;
        let root = git_tree_sha(&[TreeItem {
            name: "Cargo.toml".into(),
            mode: "100755".into(),
            sha: old_blob.clone(),
        }])?;
        let api = Api {
            sha: root.clone(),
            blob: old_blob,
        };
        let trees = GitHubTrees {
            api: &api,
            repository: "owner/project",
        };
        let source = "a".repeat(40);
        let intent = velnor_actions_contract::config::CrateReleaseIntent {
            schema: 1,
            repository: "owner/project".into(),
            default_branch: "main".into(),
            environment: "release".into(),
            required_workflow_path: ".github/workflows/ci.yml".into(),
            trusted_reviewer_id: 1,
            rust_version: "1.90.0".into(),
            generator_version: "0.1.73".into(),
            generator_source_sha: "b".repeat(40),
            generator_manifest_sha256: "c".repeat(64),
            release_plz_version: "0.4.3".into(),
            runner: "ubuntu-24.04".into(),
            authentication:
                velnor_actions_contract::config::ReleaseAuthentication::TrustedPublishing,
            manifest_path: "Cargo.toml".into(),
            pr_write_paths: vec!["Cargo.toml".into()],
            registry: "crates-io".into(),
            packages: vec!["sample".into()],
            publish_features: BTreeMap::new(),
            new_crates: None,
            tag_template: "{{ crate }}-v{{ version }}".into(),
            recovery: None,
            journal_predecessor: None,
        };
        let mut artifact = crate::crate_release_pr_artifact::PreparationArtifact {
            schema: 1,
            binding: crate::crate_release_pr_artifact::PreparationBinding {
                repository: intent.repository.clone(),
                workflow_sha: source.clone(),
                source_sha: source.clone(),
                run_id: 42,
                run_attempt: 1,
            },
            files: vec![crate::crate_release_pr_artifact::PreparedFile {
                path: "Cargo.toml".into(),
                sha256: crate::generator::sha256_hex(b"new"),
                bytes: b"new".to_vec(),
            }],
        };
        let result = build_plan(&trees, &intent, &source, &root, &artifact)?;
        assert_eq!(result.branch, "release-plz-42-1");
        assert_eq!(result.changes["Cargo.toml"].0, "100755");
        let changed_sha = crate::release_policy_admission::git_blob_sha1(b"new")?;
        assert_eq!(
            result.expected_tree,
            git_tree_sha(&[TreeItem {
                name: "Cargo.toml".into(),
                mode: "100755".into(),
                sha: changed_sha
            }])?
        );
        artifact.files[0].path = "unauthorized/Cargo.toml".into();
        assert!(build_plan(&trees, &intent, &source, &root, &artifact).is_err());
        Ok(())
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

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "The complete HTTP exchange is visible in one fixture"
    )]
    fn native_adapter_creates_and_reconciles_through_github_http() -> Result<(), OrchestratorError>
    {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
            time::{Duration, Instant},
        };
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        listener.set_nonblocking(true).expect("nonblocking");
        let address = listener.local_addr().expect("address");
        let plan = plan();
        let expected = self::plan();
        let worker = thread::spawn(move || {
            let mut branch = false;
            let mut pull = false;
            let mut requests = Vec::new();
            let mut idle = Instant::now();
            loop {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => {
                        idle = Instant::now();
                        value
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && idle.elapsed() < Duration::from_secs(2) =>
                    {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("accept: {e}"),
                };
                stream.set_nonblocking(false).expect("blocking stream");
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .expect("timeout");
                let mut data = Vec::new();
                let mut buffer = [0; 8192];
                loop {
                    let count = stream.read(&mut buffer).expect("read");
                    assert!(count > 0);
                    data.extend_from_slice(&buffer[..count]);
                    if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&data[..end]).to_ascii_lowercase();
                        let len = headers
                            .lines()
                            .find_map(|v| {
                                v.strip_prefix("content-length: ")
                                    .and_then(|n| n.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if data.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(data).expect("utf8");
                let first = request.lines().next().expect("request").to_owned();
                let path = first.split_whitespace().nth(1).expect("path");
                let method = first.split_whitespace().next().expect("method");
                let is_post = method == "POST";
                let body = if path.contains("/git/ref/heads/") {
                    serde_json::json!({"ref":format!("refs/heads/{}", expected.base),"object":{"type":"commit","sha":expected.source_sha}})
                } else if path.contains("/git/matching-refs/heads/") {
                    if branch {
                        serde_json::json!([{"ref":format!("refs/heads/{}", expected.branch),"object":{"type":"commit","sha":"d".repeat(40)}}])
                    } else {
                        serde_json::json!([])
                    }
                } else if path.contains("/commits/") && path.contains("/pulls?") {
                    serde_json::json!([])
                } else if path.contains("/git/commits/") {
                    let email = format!(
                        "{}+github-actions[bot]@users.noreply.github.com",
                        expected.author_id
                    );
                    serde_json::json!({"sha":"d".repeat(40),"tree":{"sha":expected.expected_tree},"parents":[{"sha":expected.source_sha}],"message":expected.message,"author":{"name":"github-actions[bot]","email":email},"committer":{"name":"github-actions[bot]","email":email}})
                } else if path.contains("/pulls?state=open&") {
                    serde_json::json!([])
                } else if path.contains("/pulls?") {
                    if pull {
                        serde_json::json!([{"number":17,"base":{"repo":{"full_name":expected.repository},"ref":expected.base},"head":{"repo":{"full_name":expected.repository},"ref":expected.branch,"sha":"d".repeat(40)},"user":{"id":expected.author_id},"state":"open","title":expected.title,"body":expected.body}])
                    } else {
                        serde_json::json!([])
                    }
                } else if is_post && path.ends_with("/git/blobs") {
                    serde_json::json!({"sha":crate::release_policy_admission::git_blob_sha1(b"new content").expect("blob")})
                } else if is_post && path.ends_with("/git/trees") {
                    serde_json::json!({"sha":expected.expected_tree})
                } else if is_post && path.ends_with("/git/commits") {
                    serde_json::json!({"sha":"d".repeat(40)})
                } else if is_post && path.ends_with("/git/refs") {
                    branch = true;
                    serde_json::json!({})
                } else if is_post && path.ends_with("/pulls") {
                    pull = true;
                    serde_json::json!({})
                } else {
                    panic!("unexpected request: {first}")
                };
                requests.push(first);
                let encoded = body.to_string();
                write!(
                    stream,
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    if is_post { "201 Created" } else { "200 OK" },
                    encoded.len(),
                    encoded
                )
                .expect("write");
            }
            requests
        });
        let github = crate::crate_release_github::NativeGitHubAdmissionRead {
            reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&format!(
                "http://{address}"
            ))?,
        };
        let writer = NativeWriter { github: &github };
        let result = create_or_reconcile(&writer, &plan);
        drop(github);
        let requests = worker.join().expect("worker");
        assert_eq!(result?, 17);
        assert_eq!(
            requests.iter().filter(|s| s.starts_with("POST ")).count(),
            5
        );
        assert!(
            requests
                .iter()
                .all(|s| s.starts_with("GET ") || s.starts_with("POST "))
        );
        Ok(())
    }

    #[test]
    fn rejects_existing_release_pr_on_later_page_and_api_failures() -> Result<(), OrchestratorError>
    {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
            time::Duration,
        };
        let first: Vec<_> = (0..100).map(|i| serde_json::json!({"head":{"ref":format!("feature-{i}"),"repo":{"full_name":"owner/project"}}})).collect();
        let replies = vec![
            ("200 OK", serde_json::json!(first)),
            (
                "200 OK",
                serde_json::json!([{"head":{"ref":"release-plz-another","repo":{"full_name":"other/fork"}}}]),
            ),
            ("403 Forbidden", serde_json::json!({"message":"denied"})),
        ];
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        let address = listener.local_addr().expect("address");
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, response) in replies {
                let (mut stream, _) = listener.accept().expect("accept");
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .expect("timeout");
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    let count = stream.read(&mut buffer).expect("read");
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                requests.push(
                    String::from_utf8(bytes)
                        .expect("utf8")
                        .lines()
                        .next()
                        .expect("request")
                        .to_owned(),
                );
                let body = response.to_string();
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .expect("write");
            }
            requests
        });
        let github = crate::crate_release_github::NativeGitHubAdmissionRead {
            reader: crate::crate_release_http::GitHubAdmissionHttp::test_client(&format!(
                "http://{address}"
            ))?,
        };
        let writer = NativeWriter { github: &github };
        assert!(
            matches!(writer.require_no_other_release_pr(&plan()), Err(OrchestratorError::Internal { problem }) if problem == "release_pr_existing")
        );
        assert!(
            matches!(writer.require_no_other_release_pr(&plan()), Err(OrchestratorError::Internal { problem }) if problem == "crate_release_http_response")
        );
        let requests = worker.join().expect("worker");
        assert!(requests[0].contains("page=1"));
        assert!(requests[1].contains("page=2"));
        assert!(requests.iter().all(|request| request.starts_with("GET ")));
        Ok(())
    }

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
}

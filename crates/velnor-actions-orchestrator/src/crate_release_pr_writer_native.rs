// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    BTreeMap, Commit, OrchestratorError, Plan, PullRequest, TreeEntry, WriterIo, internal,
    valid_sha,
};

pub(super) struct NativeWriter<'a> {
    pub(super) github: &'a crate::crate_release_github::NativeGitHubAdmissionRead,
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

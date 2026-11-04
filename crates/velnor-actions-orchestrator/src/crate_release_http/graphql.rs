// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubAdmissionHttp, JSON_LIMIT, OrchestratorError, internal, parse_json};
use serde_json::Value;

const THREADS: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100,after:$cursor){nodes{id isResolved isOutdated} pageInfo{hasNextPage endCursor}}}}}";

impl GitHubAdmissionHttp {
    pub fn qualification_review_threads(
        &self,
        repository: &str,
        number: u32,
        cursor: Option<&str>,
    ) -> Result<Value, OrchestratorError> {
        let Some((owner, name)) = repository.split_once('/') else {
            return Err(internal("qualification_repository_invalid"));
        };
        if !velnor_actions_contract::config::is_valid_github_owner(owner)
            || !velnor_actions_contract::config::is_valid_github_repo_name(name)
            || number == 0
            || cursor.is_some_and(|s| {
                s.is_empty() || s.len() > 512 || s.bytes().any(|b| !b.is_ascii_graphic())
            })
        {
            return Err(internal("qualification_graphql_input"));
        }
        let remaining = self
            .deadline
            .saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(internal("qualification_graphql_deadline"));
        }
        let body = serde_json::to_vec(&serde_json::json!({"query":THREADS,"variables":{"owner":owner,"name":name,"number":number,"cursor":cursor}}))
            .map_err(|_| internal("qualification_graphql_request"))?;
        let response = crate::api::github_evidence_http::agent()?
            .post(format!("{}/graphql", self.origin))
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("Accept-Encoding", "identity")
            .header("Content-Type", "application/json")
            .header("User-Agent", "velnor-actions-qualification")
            .config()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .send(body.as_slice())
            .map_err(|_| internal("qualification_graphql_transport"))?;
        if response.status().as_u16() != 200 {
            return Err(internal("qualification_graphql_status"));
        }
        let value = parse_json(response, self.deadline, JSON_LIMIT)?;
        if value.get("errors").is_some() {
            return Err(internal("qualification_graphql_errors"));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{client, reply, server};

    #[test]
    fn fixed_query_reads_threads_without_accepting_mutations()
    -> Result<(), Box<dyn std::error::Error>> {
        let (origin, worker) = server(vec![reply(
            "200 OK",
            r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
        )]);
        let api = client(origin);
        let value = api.qualification_review_threads("owner/repo", 7, None)?;
        assert!(value["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"].is_array());
        let requests = worker.join().map_err(|_| "server")?;
        assert!(requests[0].starts_with("POST /graphql "));
        assert!(requests[0].contains("reviewThreads"));
        assert!(!requests[0].contains("mutation"));
        assert!(
            api.qualification_review_threads("owner/invalid/repo", 7, None)
                .is_err()
        );
        assert!(
            api.qualification_review_threads("owner/repo", 0, None)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn graphql_errors_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
        let (origin, worker) = server(vec![reply(
            "200 OK",
            r#"{"errors":[{"message":"denied"}]}"#,
        )]);
        assert!(
            client(origin)
                .qualification_review_threads("owner/repo", 7, None)
                .is_err()
        );
        worker.join().map_err(|_| "server")?;
        Ok(())
    }
}

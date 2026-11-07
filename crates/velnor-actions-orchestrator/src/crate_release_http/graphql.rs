// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{GitHubAdmissionHttp, JSON_LIMIT, OrchestratorError, internal, parse_json};
use serde_json::Value;

const THREADS: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100,after:$cursor){nodes{id isResolved isOutdated} pageInfo{hasNextPage endCursor}}}}}";

const OPINIONS: &str = "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){latestOpinionatedReviews(first:100,after:$cursor){nodes{id state submittedAt author{__typename ... on User{databaseId} ... on Bot{databaseId}}} pageInfo{hasNextPage endCursor}}}}}";

impl GitHubAdmissionHttp {
    pub fn qualification_review_threads(
        &self,
        repository: &str,
        number: u32,
        cursor: Option<&str>,
    ) -> Result<Value, OrchestratorError> {
        let value = self.qualification_graphql_page(repository, number, cursor, THREADS)?;
        validate_page(&value, "reviewThreads")?;
        Ok(value)
    }

    pub fn qualification_opinionated_reviews(
        &self,
        repository: &str,
        number: u32,
        cursor: Option<&str>,
    ) -> Result<Value, OrchestratorError> {
        let value = self.qualification_graphql_page(repository, number, cursor, OPINIONS)?;
        validate_page(&value, "latestOpinionatedReviews")?;
        Ok(value)
    }

    fn qualification_graphql_page(
        &self,
        repository: &str,
        number: u32,
        cursor: Option<&str>,
        query: &'static str,
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
        self.fixed_graphql(
            query,
            &serde_json::json!({"owner":owner,"name":name,"number":number,"cursor":cursor}),
        )
    }

    pub(crate) fn qualification_provider_graphql(
        &self,
        query: crate::tap_merge::review_completion::provider::ProviderQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        use crate::tap_merge::review_completion::provider::ProviderQuery;
        let mut variables = serde_json::Map::new();
        for (key, value) in fields {
            if !matches!(*key, "id" | "cursor" | "owner" | "name" | "number")
                || value.is_empty()
                || value.len() > 512
                || value.bytes().any(|b| !b.is_ascii_graphic())
                || variables
                    .insert((*key).to_owned(), Value::String(value.clone()))
                    .is_some()
            {
                return Err(internal("qualification_provider_fields"));
            }
        }
        if matches!(query, ProviderQuery::DeletedComments) {
            let owner = variables
                .get("owner")
                .and_then(Value::as_str)
                .ok_or_else(|| internal("qualification_provider_fields"))?;
            let name = variables
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| internal("qualification_provider_fields"))?;
            let raw_number = variables
                .get("number")
                .and_then(Value::as_str)
                .ok_or_else(|| internal("qualification_provider_fields"))?;
            let number = raw_number
                .parse::<u32>()
                .ok()
                .filter(|n| *n > 0 && n.to_string() == raw_number)
                .ok_or_else(|| internal("qualification_provider_fields"))?;
            if !velnor_actions_contract::config::is_valid_github_owner(owner)
                || !velnor_actions_contract::config::is_valid_github_repo_name(name)
                || variables.contains_key("id")
            {
                return Err(internal("qualification_provider_fields"));
            }
            variables.insert("number".into(), Value::Number(number.into()));
        } else if !variables.contains_key("id")
            || variables.contains_key("owner")
            || variables.contains_key("name")
            || variables.contains_key("number")
            || (variables.contains_key("cursor") && !matches!(query, ProviderQuery::ThreadComments))
        {
            return Err(internal("qualification_provider_fields"));
        }
        self.fixed_graphql(query.query(), &Value::Object(variables))
    }

    fn fixed_graphql(
        &self,
        query: &'static str,
        variables: &Value,
    ) -> Result<Value, OrchestratorError> {
        let timeout = velnor_actions_mise::CheckDeadline::cap_to_scope(self.timeout)
            .map_err(|_| internal("qualification_graphql_deadline"))?;
        let deadline = std::time::Instant::now() + timeout;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(internal("qualification_graphql_deadline"));
        }
        let body = serde_json::to_vec(&serde_json::json!({"query":query,"variables":variables}))
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
        let value = parse_json(response, deadline, JSON_LIMIT)?;
        if value.get("errors").is_some() {
            return Err(internal("qualification_graphql_errors"));
        }
        Ok(value)
    }
}

fn validate_page(value: &Value, connection: &str) -> Result<(), OrchestratorError> {
    let invalid = || internal("qualification_graphql_page");
    let page = value
        .pointer(&format!("/data/repository/pullRequest/{connection}"))
        .ok_or_else(invalid)?;
    let nodes = page["nodes"]
        .as_array()
        .filter(|items| items.len() <= 100)
        .ok_or_else(invalid)?;
    if nodes.iter().any(|node| {
        node["id"]
            .as_str()
            .is_none_or(|id| id.is_empty() || id.len() > 512)
            || if connection == "reviewThreads" {
                node["isResolved"].as_bool().is_none() || node["isOutdated"].as_bool().is_none()
            } else {
                node["state"].as_str().is_none_or(str::is_empty)
                    || node["submittedAt"].as_str().is_none_or(str::is_empty)
                    || !matches!(node["author"]["__typename"].as_str(), Some("User" | "Bot"))
                    || node["author"]["databaseId"]
                        .as_u64()
                        .is_none_or(|id| id == 0)
            }
    }) {
        return Err(invalid());
    }
    let more = page["pageInfo"]["hasNextPage"]
        .as_bool()
        .ok_or_else(invalid)?;
    let cursor = &page["pageInfo"]["endCursor"];
    if more
        && cursor.as_str().is_none_or(|s| {
            s.is_empty() || s.len() > 512 || s.bytes().any(|b| !b.is_ascii_graphic())
        })
    {
        return Err(invalid());
    }
    if !(more
        || cursor.is_null()
        || cursor
            .as_str()
            .is_some_and(|s| s.len() <= 512 && s.bytes().all(|b| b.is_ascii_graphic())))
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
#[path = "graphql_tests.rs"]
mod tests;

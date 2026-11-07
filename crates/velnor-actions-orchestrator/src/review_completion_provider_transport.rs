// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::super::github::TapMergeGitHub;
use super::{EDITOR, REVIEW_EDITOR, dispositions, findings};
use crate::OrchestratorError;
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub enum ProviderApp {
    Codex,
    Sonar,
}

impl ProviderApp {
    pub(crate) fn route(self) -> &'static str {
        match self {
            Self::Codex => "apps/chatgpt-codex-connector",
            Self::Sonar => "apps/sonarqubecloud",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProviderQuery {
    IssueEditor,
    BotReviewEditor,
    HumanReviewEditor,
    ThreadComments,
    DeletedComments,
}

impl ProviderQuery {
    pub(crate) fn query(self) -> &'static str {
        match self {
            Self::IssueEditor => EDITOR,
            Self::BotReviewEditor => REVIEW_EDITOR,
            Self::HumanReviewEditor => dispositions::USER_REVIEW_EDITOR,
            Self::ThreadComments => findings::QUERY,
            Self::DeletedComments => {
                "query($owner:String!,$name:String!,$number:Int!,$cursor:String){repository(owner:$owner,name:$name){fullDatabaseId nameWithOwner pullRequest(number:$number){number timelineItems(first:100,after:$cursor,itemTypes:[COMMENT_DELETED_EVENT]){nodes{... on CommentDeletedEvent{id createdAt}} pageInfo{hasNextPage endCursor}}}}}"
            }
        }
    }
}

pub trait ProviderRead {
    fn provider_app(&self, app: ProviderApp) -> Result<Value, OrchestratorError>;
    fn provider_graphql(
        &self,
        query: ProviderQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError>;
    fn provider_check_runs(
        &self,
        repository: &str,
        head: &str,
    ) -> Result<Vec<Value>, OrchestratorError>;
}

impl ProviderRead for TapMergeGitHub {
    fn provider_app(&self, app: ProviderApp) -> Result<Value, OrchestratorError> {
        self.get(app.route())
    }
    fn provider_graphql(
        &self,
        query: ProviderQuery,
        fields: &[(&str, String)],
    ) -> Result<Value, OrchestratorError> {
        self.graphql(query.query(), fields)
    }
    fn provider_check_runs(
        &self,
        repository: &str,
        head: &str,
    ) -> Result<Vec<Value>, OrchestratorError> {
        super::super::super::observation::check_runs(self, repository, head)
    }
}

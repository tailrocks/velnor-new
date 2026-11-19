// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{PrivateSources, RepoPath, StepKind};

use crate::release_steps::{APP_TOKEN_COMMIT, APP_TOKEN_VERSION, shell_word};
use crate::rules::{APP_TOKEN_ACTION, RenderedStep, Value};

pub(crate) const TOKEN_STEP: &str = "private-token";
pub(crate) const CHECK_STEP: &str = "private-check";
const TOKEN_ALLOWED: &str = "${{ (github.event_name == 'merge_group' && steps.private-check.outputs.allowed == 'true') || (github.event_name != 'merge_group' && (github.event_name != 'pull_request' || (github.event.pull_request.head.repo.full_name == github.repository && !endsWith(github.actor, '[bot]') && !endsWith(github.event.pull_request.user.login, '[bot]')))) }}";

pub(crate) fn render(kind: &StepKind) -> RenderedStep {
    match kind {
        StepKind::PrivateCheck { .. } => check(),
        StepKind::PrivateToken { sources, .. } => token(sources),
        StepKind::PrivateFetch { manifests, .. } => fetch(manifests),
        _ => RenderedStep::default(),
    }
}

fn check() -> RenderedStep {
    RenderedStep {
        id: Some(CHECK_STEP.to_owned()),
        name: "Check that the merge queue entry is not from a fork".to_owned(),
        condition: Some("${{ github.event_name == 'merge_group' }}".to_owned()),
        env: vec![
            (
                "GITHUB_TOKEN".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            ),
            (
                "VELNOR_API_URL".to_owned(),
                Value::Expr("${{ github.api_url }}".to_owned()),
            ),
            (
                "VELNOR_MERGE_GROUP_HEAD_REF".to_owned(),
                Value::Expr("${{ github.event.merge_group.head_ref }}".to_owned()),
            ),
        ],
        run: Some("velnor-ci private-token-allowed".to_owned()),
        ..RenderedStep::default()
    }
}

fn token(sources: &PrivateSources) -> RenderedStep {
    RenderedStep {
        id: Some(TOKEN_STEP.to_owned()),
        name: "Create a read-only token for the private repositories".to_owned(),
        condition: Some(TOKEN_ALLOWED.to_owned()),
        uses: Some(format!("{APP_TOKEN_ACTION}@{APP_TOKEN_COMMIT}")),
        uses_comment: Some(APP_TOKEN_VERSION.to_owned()),
        with: vec![
            (
                "client-id".to_owned(),
                Value::Expr(format!("${{{{ vars.{} }}}}", sources.app_id)),
            ),
            (
                "private-key".to_owned(),
                Value::Expr(format!("${{{{ secrets.{} }}}}", sources.private_key)),
            ),
            ("owner".to_owned(), Value::Text(sources.owner.clone())),
            (
                "repositories".to_owned(),
                Value::Text(sources.repositories.join("\n")),
            ),
            (
                "permission-contents".to_owned(),
                Value::Text("read".to_owned()),
            ),
        ],
        ..RenderedStep::default()
    }
}

fn fetch(manifests: &[RepoPath]) -> RenderedStep {
    let arguments: Vec<String> = manifests
        .iter()
        .map(|manifest| format!("--manifest-path {}", shell_word(manifest.as_str())))
        .collect();
    RenderedStep {
        name: "Fetch the dependencies, then drop the token".to_owned(),
        env: vec![
            (
                "VELNOR_PRIVATE_TOKEN".to_owned(),
                Value::Expr(format!("${{{{ steps.{TOKEN_STEP}.outputs.token }}}}")),
            ),
            (
                "VELNOR_API_URL".to_owned(),
                Value::Expr("${{ github.api_url }}".to_owned()),
            ),
            (
                "VELNOR_SERVER_URL".to_owned(),
                Value::Expr("${{ github.server_url }}".to_owned()),
            ),
        ],
        run: Some(format!("velnor-ci fetch {}", arguments.join(" "))),
        ..RenderedStep::default()
    }
}

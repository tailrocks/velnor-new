// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{PrivateSources, RepoPath};

use crate::release_steps::{APP_TOKEN_COMMIT, APP_TOKEN_VERSION, shell_word};
use crate::rules::{APP_TOKEN_ACTION, RenderedStep, Value};

pub(crate) const TOKEN_STEP: &str = "private-token";
const FORK_FREE: &str = "${{ github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository }}";

pub(crate) fn token(sources: &PrivateSources) -> RenderedStep {
    RenderedStep {
        id: Some(TOKEN_STEP.to_owned()),
        name: "Create a read-only token for the private repositories".to_owned(),
        condition: Some(FORK_FREE.to_owned()),
        uses: Some(format!("{APP_TOKEN_ACTION}@{APP_TOKEN_COMMIT}")),
        uses_comment: Some(APP_TOKEN_VERSION.to_owned()),
        with: vec![
            (
                "app-id".to_owned(),
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
            ("skip-token-revoke".to_owned(), Value::Bool(true)),
        ],
        ..RenderedStep::default()
    }
}

pub(crate) fn fetch(manifests: &[RepoPath]) -> RenderedStep {
    let arguments: Vec<String> = manifests
        .iter()
        .map(|manifest| format!("--manifest-path {}", shell_word(manifest.as_str())))
        .collect();
    RenderedStep {
        name: "Fetch the dependencies, then drop the token".to_owned(),
        env: vec![(
            "VELNOR_PRIVATE_TOKEN".to_owned(),
            Value::Expr(format!("${{{{ steps.{TOKEN_STEP}.outputs.token }}}}")),
        )],
        run: Some(format!("velnor-ci fetch {}", arguments.join(" "))),
        ..RenderedStep::default()
    }
}

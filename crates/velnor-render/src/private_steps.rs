// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{PrivateSources, RepoPath, RootSlug, Step, StepKind};

use crate::gate::sources_step;
use crate::release_steps::{APP_TOKEN_COMMIT, APP_TOKEN_VERSION, shell_word};
use crate::rules::{APP_TOKEN_ACTION, RenderedStep, Value};
use crate::steps::RUN;

pub(crate) const TOKEN_STEP: &str = "private-token";
pub(crate) const CHECK_STEP: &str = "private-check";
pub(crate) const PRIVACY_STEP: &str = "private_repo";
const PRIVATE_REPOSITORY: &str = "steps.private_repo.outputs.private == 'true'";
macro_rules! trusted_pull_request {
    () => {
        "github.event.pull_request.head.repo.full_name == github.repository && !endsWith(github.actor, '[bot]') && !endsWith(github.event.pull_request.user.login, '[bot]')"
    };
}

const RESTORE_ALLOWED: &str = concat!(
    "(github.event_name != 'pull_request' || (",
    trusted_pull_request!(),
    "))"
);
const TOKEN_ALLOWED: &str = concat!(
    "${{ (github.event_name == 'merge_group' && steps.private-check.outputs.allowed == 'true') || (github.event_name != 'merge_group' && (github.event_name != 'pull_request' || (",
    trusted_pull_request!(),
    "))) }}"
);

pub(crate) fn render(kind: &StepKind) -> RenderedStep {
    match kind {
        StepKind::RepositoryPrivacy => privacy(),
        StepKind::PrivateCheck { .. } => check(),
        StepKind::PrivateToken { sources, .. } => token(sources),
        StepKind::PrivateFetch { manifests, .. } => fetch(manifests),
        _ => RenderedStep::default(),
    }
}

pub(crate) fn repository_conditions(step: &Step) -> Vec<String> {
    if !step.private_repo {
        return Vec::new();
    }
    let mut conditions = vec![PRIVATE_REPOSITORY.to_owned()];
    if matches!(step.kind, StepKind::CacheRestore(_)) {
        conditions.push(RESTORE_ALLOWED.to_owned());
    }
    conditions
}

pub(crate) fn decide_env(step: &Step) -> Vec<(String, Value)> {
    if step.private_repo && matches!(step.kind, StepKind::CacheDecide { .. }) {
        api_env()
    } else {
        Vec::new()
    }
}

fn api_env() -> Vec<(String, Value)> {
    vec![
        (
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ github.token }}".to_owned()),
        ),
        (
            "VELNOR_API_URL".to_owned(),
            Value::Expr("${{ github.api_url }}".to_owned()),
        ),
    ]
}

fn privacy() -> RenderedStep {
    RenderedStep {
        id: Some(PRIVACY_STEP.to_owned()),
        name: "Check that the repository is private".to_owned(),
        env: api_env(),
        run: Some("velnor-ci cache private-repo".to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn unless_hit(cached: &[RootSlug]) -> Vec<String> {
    if cached.is_empty() {
        return Vec::new();
    }
    let misses: Vec<String> = cached
        .iter()
        .map(|slug| format!("steps.{}.outputs.cache-hit != 'true'", sources_step(slug)))
        .collect();
    vec![format!("({})", misses.join(" || "))]
}

pub(crate) fn release_phase(sources: &PrivateSources, manifest: &str) -> [RenderedStep; 2] {
    [
        RenderedStep {
            condition: None,
            ..token(sources)
        },
        fetch_paths([manifest]),
    ]
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

/// The advisory DB is public, so this step runs with no token and no private config
/// (the job environment carries neither); cargo-deny then reads it with `--offline`.
pub(crate) fn advisory_fetch(configs: &[Option<RepoPath>]) -> RenderedStep {
    let run = configs
        .iter()
        .map(|config| match config {
            Some(path) => format!(
                "{RUN} cargo-deny --config {} fetch db",
                shell_word(path.as_str())
            ),
            None => format!("{RUN} cargo-deny fetch db"),
        })
        .collect::<Vec<_>>()
        .join(" && ");
    RenderedStep {
        name: "Fetch the RustSec advisory database".to_owned(),
        run: Some(run),
        ..RenderedStep::default()
    }
}

fn fetch(manifests: &[RepoPath]) -> RenderedStep {
    fetch_paths(manifests.iter().map(RepoPath::as_str))
}

fn fetch_paths<'a>(manifests: impl IntoIterator<Item = &'a str>) -> RenderedStep {
    let arguments: Vec<String> = manifests
        .into_iter()
        .map(|manifest| format!("--manifest-path {}", shell_word(manifest)))
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

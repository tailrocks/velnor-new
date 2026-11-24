// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    HelperRelease, PrivateSources, PrivateVerdict, RepoPath, RootSlug, Step, StepKind, TokenMint,
};

use crate::gate::{ON_DEFAULT_BRANCH, SAVING_EVENTS, sources_step};
use crate::release_steps::{APP_TOKEN_COMMIT, APP_TOKEN_VERSION, shell_word};
use crate::rules::{APP_TOKEN_ACTION, RenderedStep, Value};
use crate::steps::RUN;

pub(crate) const TOKEN_STEP: &str = "private-token";
pub(crate) const CHECK_STEP: &str = "private-check";
pub(crate) const PRIVACY_STEP: &str = "private_repo";
const PRIVATE_REPOSITORY: &str = "steps.private_repo.outputs.private == 'true'";
// helper-gate: first in tools-v0.1.112 via 17c0699cb6
const RELEASE_HELPER_MINT: (u64, u64, u64) = (0, 1, 112);
mod fetch_step;
pub(crate) mod release_asset_steps;

/// The condition for creating an App token: a merge queue entry that `check` allowed, or a run
/// that is not a fork pull request and not a bot's.
pub(crate) fn token_allowed(check: &str) -> String {
    format!(
        "(github.event_name == 'merge_group' && steps.{check}.outputs.allowed == 'true') || (github.event_name != 'merge_group' && (github.event_name != 'pull_request' || (github.event.pull_request.head.repo.full_name == github.repository && !endsWith(github.actor, '[bot]') && !endsWith(github.event.pull_request.user.login, '[bot]'))))"
    )
}

pub(crate) fn render(kind: &StepKind) -> RenderedStep {
    match kind {
        StepKind::RepositoryPrivacy => privacy(),
        StepKind::PrivateCheck { .. } => check(),
        StepKind::PrivateToken { sources, mint, .. } => token(sources, *mint),
        StepKind::PrivateFetch {
            manifests,
            filled,
            filled_private,
            listed,
            ..
        } => RenderedStep {
            condition: (!filled.is_empty())
                .then(|| format!("${{{{ !({}) }}}}", saving_run(*filled_private))),
            ..fetch_step::selected(manifests, *listed)
        },
        StepKind::ReleaseAssets { assets, part } => {
            crate::private_steps::release_asset_steps::render(assets, part)
        }
        _ => RenderedStep::default(),
    }
}

pub(crate) fn repository_conditions(step: &Step) -> Vec<String> {
    if !step.private_repo {
        return Vec::new();
    }
    let mut conditions = vec![PRIVATE_REPOSITORY.to_owned()];
    if matches!(step.kind, StepKind::CacheRestore(_)) {
        conditions.push(format!("({})", token_allowed(CHECK_STEP)));
    }
    conditions
}

/// `cache decide` trusts `VELNOR_REPOSITORY_PRIVATE` alone, and a step `env` overrides
/// `$GITHUB_ENV`, so the step always sets it itself: to the privacy step's answer, or to an
/// empty string that refuses every private source.
pub(crate) fn decide_env(step: &Step) -> Vec<(String, Value)> {
    let StepKind::CacheDecide {
        private,
        sources_restore,
        ..
    } = &step.kind
    else {
        return Vec::new();
    };
    let mut env = Vec::new();
    match private {
        PrivateVerdict::Unset => {}
        PrivateVerdict::Cleared => env.push((
            "VELNOR_REPOSITORY_PRIVATE".to_owned(),
            Value::Text(String::new()),
        )),
        PrivateVerdict::Checked => env.push((
            "VELNOR_REPOSITORY_PRIVATE".to_owned(),
            Value::Expr(format!("${{{{ steps.{PRIVACY_STEP}.outputs.private }}}}")),
        )),
    }
    if let Some(lane) = sources_restore {
        env.push((
            "VELNOR_SOURCES_RESTORED".to_owned(),
            Value::Expr(format!(
                "${{{{ steps.{}.outputs.cache-hit }}}}",
                sources_step(lane)
            )),
        ));
    }
    env
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

pub(crate) fn release_mint(helper: &HelperRelease) -> TokenMint {
    if helper.version().at_least(RELEASE_HELPER_MINT) {
        TokenMint::Helper
    } else {
        TokenMint::Action
    }
}

pub(crate) fn release_phase(
    sources: &PrivateSources,
    manifest: &str,
    mint: TokenMint,
) -> [RenderedStep; 2] {
    [
        RenderedStep {
            condition: None,
            ..token(sources, mint)
        },
        fetch_step::one(manifest),
    ]
}

pub(crate) fn check() -> RenderedStep {
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

fn token(sources: &PrivateSources, mint: TokenMint) -> RenderedStep {
    let repositories: Vec<&str> = sources.repositories.iter().map(String::as_str).collect();
    RenderedStep {
        id: Some(TOKEN_STEP.to_owned()),
        name: "Create a read-only token for the private repositories".to_owned(),
        condition: Some(format!("${{{{ {} }}}}", token_allowed(CHECK_STEP))),
        ..app_token(sources, &sources.owner, &repositories, mint)
    }
}

pub(crate) fn app_token(
    sources: &PrivateSources,
    owner: &str,
    repositories: &[&str],
    mint: TokenMint,
) -> RenderedStep {
    let client_id = Value::Expr(format!("${{{{ vars.{} }}}}", sources.app_id));
    let private_key = Value::Expr(format!("${{{{ secrets.{} }}}}", sources.private_key));
    if mint == TokenMint::Helper {
        let names: Vec<String> = repositories
            .iter()
            .map(|name| format!("--repository={}", shell_word(name)))
            .collect();
        return RenderedStep {
            env: vec![
                ("VELNOR_APP_CLIENT_ID".to_owned(), client_id),
                ("VELNOR_APP_PRIVATE_KEY".to_owned(), private_key),
                (
                    "VELNOR_API_URL".to_owned(),
                    Value::Expr("${{ github.api_url }}".to_owned()),
                ),
            ],
            run: Some(format!(
                "velnor-ci app-token --owner={} {}",
                shell_word(owner),
                names.join(" ")
            )),
            ..RenderedStep::default()
        };
    }
    RenderedStep {
        uses: Some(format!("{APP_TOKEN_ACTION}@{APP_TOKEN_COMMIT}")),
        uses_comment: Some(APP_TOKEN_VERSION.to_owned()),
        with: vec![
            ("client-id".to_owned(), client_id),
            ("private-key".to_owned(), private_key),
            ("owner".to_owned(), Value::Text(owner.to_owned())),
            (
                "repositories".to_owned(),
                Value::Text(repositories.join("\n")),
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

fn saving_run(private: bool) -> String {
    let saving = format!("{ON_DEFAULT_BRANCH} && {SAVING_EVENTS}");
    if private {
        format!("{saving} && {PRIVATE_REPOSITORY}")
    } else {
        saving
    }
}

pub(crate) fn filled_fetch(filled: &[RepoPath], private: bool, listed: bool) -> Vec<RenderedStep> {
    if filled.is_empty() {
        return Vec::new();
    }
    vec![RenderedStep {
        name: "Fetch the dependencies of every root, then drop the token".to_owned(),
        condition: Some(format!("${{{{ {} }}}}", saving_run(private))),
        ..fetch_step::every(filled, listed)
    }]
}

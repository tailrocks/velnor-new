// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{PrivateSources, ReleaseAsset, ReleaseAssetPart};

use crate::private_steps::token_allowed;
use crate::release_steps::{APP_TOKEN_COMMIT, APP_TOKEN_VERSION, shell_word};
use crate::rules::{APP_TOKEN_ACTION, RenderedStep, Value};
use crate::steps::{CACHE_RESTORE, CACHE_SAVE};

pub(crate) const TOKEN_STEP: &str = "release-assets-token";
const CHECK_STEP: &str = "release-assets-check";

pub(crate) fn render(assets: &[ReleaseAsset], part: &ReleaseAssetPart) -> RenderedStep {
    match part {
        ReleaseAssetPart::Restore(index) => restore(&assets[*index], *index),
        ReleaseAssetPart::Check => check(assets),
        ReleaseAssetPart::Token(sources) => token(sources, assets),
        ReleaseAssetPart::Fetch => fetch(assets),
        ReleaseAssetPart::Save(index) => save(&assets[*index], *index),
    }
}

fn cache_step(index: usize) -> String {
    format!("release-asset-{index}")
}

fn any_miss(assets: &[ReleaseAsset]) -> String {
    let misses: Vec<String> = (0..assets.len())
        .map(|index| format!("steps.{}.outputs.cache-hit != 'true'", cache_step(index)))
        .collect();
    format!("({})", misses.join(" || "))
}

fn cache_with(asset: &ReleaseAsset) -> Vec<(String, Value)> {
    vec![
        (
            "path".to_owned(),
            Value::Text(format!("{}/{}", asset.dest, asset.name)),
        ),
        (
            "key".to_owned(),
            Value::Text(format!("release-asset-{}", asset.sha256)),
        ),
    ]
}

fn restore(asset: &ReleaseAsset, index: usize) -> RenderedStep {
    RenderedStep {
        id: Some(cache_step(index)),
        name: format!("Restore the release asset {}", asset.name),
        uses: Some(CACHE_RESTORE.to_owned()),
        with: cache_with(asset),
        ..RenderedStep::default()
    }
}

fn save(asset: &ReleaseAsset, index: usize) -> RenderedStep {
    RenderedStep {
        name: format!("Save the verified release asset {}", asset.name),
        condition: Some(format!(
            "${{{{ steps.{}.outputs.cache-hit != 'true' }}}}",
            cache_step(index)
        )),
        continue_on_error: true,
        uses: Some(CACHE_SAVE.to_owned()),
        with: cache_with(asset),
        ..RenderedStep::default()
    }
}

fn check(assets: &[ReleaseAsset]) -> RenderedStep {
    let mut step = crate::private_steps::check();
    step.id = Some(CHECK_STEP.to_owned());
    step.condition = Some(format!(
        "${{{{ github.event_name == 'merge_group' && {} }}}}",
        any_miss(assets)
    ));
    step
}

fn token(sources: &PrivateSources, assets: &[ReleaseAsset]) -> RenderedStep {
    let mut repositories: Vec<&str> = Vec::new();
    for asset in assets {
        if !repositories.contains(&asset.repo.as_str()) {
            repositories.push(&asset.repo);
        }
    }
    RenderedStep {
        id: Some(TOKEN_STEP.to_owned()),
        name: "Create a read-only token for the private release assets".to_owned(),
        condition: Some(format!(
            "${{{{ {} && ({}) }}}}",
            any_miss(assets),
            token_allowed(CHECK_STEP)
        )),
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
            (
                "owner".to_owned(),
                Value::Text(assets[0].owner.clone()),
            ),
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

fn fetch(assets: &[ReleaseAsset]) -> RenderedStep {
    let arguments: Vec<String> = assets
        .iter()
        .map(|asset| format!("--asset {}", shell_word(&asset.argument())))
        .collect();
    RenderedStep {
        name: "Fetch the release assets, then drop the token".to_owned(),
        env: vec![
            (
                "VELNOR_PRIVATE_TOKEN".to_owned(),
                Value::Expr(format!("${{{{ steps.{TOKEN_STEP}.outputs.token }}}}")),
            ),
            (
                "VELNOR_API_URL".to_owned(),
                Value::Expr("${{ github.api_url }}".to_owned()),
            ),
        ],
        run: Some(format!("velnor-ci fetch-assets {}", arguments.join(" "))),
        ..RenderedStep::default()
    }
}

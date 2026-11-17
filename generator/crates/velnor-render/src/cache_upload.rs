// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache::render_key;
use crate::gate::sources_step;
use crate::rules::{RenderedStep, Value};
use velnor_model::{CacheKey, JobId, StepKind, TrimFamily};

pub(crate) const SOURCES_KEY_OUTPUT: &str = "sources_key";
const TARGET_KEY_OUTPUT: &str = "target_key";

pub(crate) fn key_output(family: TrimFamily) -> &'static str {
    match family {
        TrimFamily::Target => TARGET_KEY_OUTPUT,
        TrimFamily::Sources => SOURCES_KEY_OUTPUT,
    }
}

pub(crate) fn record(kind: &StepKind, saved: &RenderedStep) -> Option<RenderedStep> {
    let StepKind::CacheSave(key) = kind else {
        return None;
    };
    Some(RenderedStep {
        id: Some(key_id(key)?),
        name: format!("Record the {} cache key", key.family()),
        condition: saved.condition.clone(),
        env: vec![("CACHE_KEY".to_owned(), Value::Expr(render_key(key).key))],
        run: Some("echo \"key=$CACHE_KEY\" >> \"$GITHUB_OUTPUT\"".to_owned()),
        ..RenderedStep::default()
    })
}

pub(crate) fn key_id(key: &CacheKey) -> Option<String> {
    match key {
        CacheKey::Target { .. } => Some(TARGET_KEY_OUTPUT.to_owned()),
        CacheKey::Sources { root, .. } => Some(format!("{}_key", sources_step(root.slug()))),
        _ => None,
    }
}

fn verify_id(lane: &JobId, family: TrimFamily) -> String {
    format!("verify-{lane}-{}", key_output(family))
}

pub(crate) fn verify(lane: &JobId, family: TrimFamily) -> RenderedStep {
    let key = format!("needs.{lane}.outputs.{}", key_output(family));
    RenderedStep {
        id: Some(verify_id(lane, family)),
        name: format!(
            "Verify the {lane} {} cache upload",
            key_output(family).trim_end_matches("_key")
        ),
        condition: Some(format!("${{{{ {key} != '' }}}}")),
        continue_on_error: true,
        env: vec![
            (
                "CACHE_KEY".to_owned(),
                Value::Expr(format!("${{{{ {key} }}}}")),
            ),
            (
                "GITHUB_TOKEN".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            ),
        ],
        run: Some("velnor-ci cache verify --key \"$CACHE_KEY\"".to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn result(uploads: &[(JobId, TrimFamily)]) -> RenderedStep {
    let terms = uploads
        .iter()
        .map(|(lane, family)| format!("steps.{}.outcome != 'failure'", verify_id(lane, *family)))
        .collect::<Vec<_>>()
        .join(" && ");
    RenderedStep {
        name: "Require verified cache uploads".to_owned(),
        env: vec![(
            "CACHE_OK".to_owned(),
            Value::Expr(format!("${{{{ {terms} }}}}")),
        )],
        run: Some("velnor-ci cache check --success \"$CACHE_OK\"".to_owned()),
        ..RenderedStep::default()
    }
}

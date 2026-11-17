// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache::render_key;
use crate::gate::sources_step;
use crate::rules::{RenderedStep, Value};
use velnor_model::{CacheKey, JobId, StepKind};

pub(crate) fn verification(kind: &StepKind, saved: &RenderedStep) -> Option<RenderedStep> {
    let StepKind::CacheSave(key) = kind else {
        return None;
    };
    let id = verification_id(key)?;
    Some(RenderedStep {
        id: Some(id),
        name: format!("Verify the {} cache upload", key.family()),
        condition: saved.condition.clone(),
        continue_on_error: true,
        env: vec![
            ("CACHE_KEY".to_owned(), Value::Expr(render_key(key).key)),
            (
                "GITHUB_TOKEN".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            ),
        ],
        run: Some("velnor-ci cache verify --key \"$CACHE_KEY\"".to_owned()),
        ..RenderedStep::default()
    })
}

pub(crate) fn verification_id(key: &CacheKey) -> Option<String> {
    match key {
        CacheKey::Target { .. } => Some("target_verified".to_owned()),
        CacheKey::Sources { root, .. } => Some(format!("{}_verified", sources_step(root.slug()))),
        _ => None,
    }
}

pub(crate) fn result(lanes: &[JobId]) -> RenderedStep {
    let terms = lanes
        .iter()
        .map(|lane| format!("needs.{lane}.outputs.cache_ok != 'false'"))
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

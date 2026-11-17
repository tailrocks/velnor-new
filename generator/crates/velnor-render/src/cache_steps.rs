// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::CacheKey;

use crate::cache::{path_text, render_key};
use crate::gate::{TOOLS_STEP, sources_save_step, sources_step};
use crate::rules::{RenderedStep, Value};

pub(crate) fn cache_step(
    action: &str,
    verb: &str,
    key: &CacheKey,
    fallbacks: bool,
) -> RenderedStep {
    let rendered = render_key(key);
    let paths: Vec<String> = key.paths().iter().map(path_text).collect();
    let mut with = vec![
        ("path".to_owned(), Value::Text(paths.join("\n"))),
        ("key".to_owned(), Value::Expr(rendered.key)),
    ];
    if fallbacks && !rendered.restore_keys.is_empty() {
        with.push((
            "restore-keys".to_owned(),
            Value::Expr(rendered.restore_keys.join("\n")),
        ));
    }
    RenderedStep {
        id: match key {
            CacheKey::Tools { .. } if fallbacks => Some(TOOLS_STEP.to_owned()),
            CacheKey::Sources { root, .. } if fallbacks => Some(sources_step(root.slug())),
            CacheKey::Target { root, .. } if fallbacks => Some(format!("target_{}", root.slug())),
            CacheKey::Sources { root, .. } => Some(sources_save_step(root.slug())),
            _ => None,
        },
        name: format!("{verb} the {} cache", key.family()),
        uses: Some(action.to_owned()),
        continue_on_error: !fallbacks && crate::cache_upload::key_id(key).is_some(),
        with,
        ..RenderedStep::default()
    }
}

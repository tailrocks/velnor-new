// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CacheKey, ToolsAnchor};

use crate::rules::{RenderedStep, Value};

pub(super) const ANCHOR_STEP: &str = "toolsanchor";
pub(super) const READ_STEP: &str = "toolsread";
const ANCHOR_DIR: &str = "${{ runner.temp }}/velnor-tools-anchor";
const ANCHOR_RUN: &str = "${{ github.run_id }}-${{ github.job }}";

fn anchor_prefix() -> String {
    crate::cache::render_key(&CacheKey::Tools {
        host: velnor_model::Host,
        anchor: ToolsAnchor::Unanchored,
    })
    .key
    .replacen("velnor-tools-", "velnor-tools-anchor-", 1)
}

pub(super) fn anchor_cache_step(action: &str, verb: &str) -> RenderedStep {
    RenderedStep {
        id: (verb == "Restore").then(|| ANCHOR_STEP.to_owned()),
        name: format!("{verb} anchor"),
        shared: true,
        uses: Some(action.to_owned()),
        with: vec![
            ("path".to_owned(), Value::Expr(ANCHOR_DIR.to_owned())),
            ("key".to_owned(), Value::Expr(anchor_prefix())),
        ],
        ..RenderedStep::default()
    }
}

pub(super) fn newest_anchor_step(action: &str, find: bool) -> RenderedStep {
    let prefix = format!("{}-", anchor_prefix());
    let mut with = vec![
        ("path".to_owned(), Value::Expr(ANCHOR_DIR.to_owned())),
        (
            "key".to_owned(),
            Value::Expr(format!("{prefix}{ANCHOR_RUN}")),
        ),
    ];
    if find {
        with.push(("restore-keys".to_owned(), Value::Expr(prefix)));
    }
    RenderedStep {
        id: find.then(|| ANCHOR_STEP.to_owned()),
        name: if find {
            "Find the anchor"
        } else {
            "Save anchor"
        }
        .to_owned(),
        shared: true,
        uses: Some(action.to_owned()),
        with,
        ..RenderedStep::default()
    }
}

pub(super) fn read_step() -> RenderedStep {
    RenderedStep {
        id: Some(READ_STEP.to_owned()),
        name: "Read the anchor".to_owned(),
        shared: true,
        env: vec![(
            "VELNOR_ANCHOR_MATCHED".to_owned(),
            Value::Expr(format!(
                "${{{{ steps.{ANCHOR_STEP}.outputs.cache-matched-key }}}}"
            )),
        )],
        run: Some("velnor-ci tools-verify read".to_owned()),
        ..RenderedStep::default()
    }
}

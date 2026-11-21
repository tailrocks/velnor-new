// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::ToolsAnchor;

use crate::rules::{RenderedStep, Value};

pub(crate) fn tools_saved_key_step(key: String) -> RenderedStep {
    let network = crate::report::cache_id(
        &velnor_model::CacheKey::Tools {
            host: velnor_model::Host,
            anchor: ToolsAnchor::Unanchored,
        },
        false,
    );
    RenderedStep {
        id: Some(crate::gate::TOOLS_SAVED_KEY_STEP.to_owned()),
        name: "Record the tools cache key".to_owned(),
        condition: Some(format!("${{{{ steps.{network}.outcome == 'success' }}}}")),
        env: vec![("CACHE_KEY".to_owned(), Value::Expr(key))],
        run: Some("echo \"key=$CACHE_KEY\" >> \"$GITHUB_OUTPUT\"".to_owned()),
        ..RenderedStep::default()
    }
}

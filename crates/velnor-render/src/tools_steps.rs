// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::RenderedStep;

pub(crate) fn tools_local_step(verb: velnor_model::ToolsLocalVerb) -> RenderedStep {
    use velnor_model::ToolsLocalVerb as Verb;
    let key = crate::cache::render_key(&velnor_model::CacheKey::Tools {
        host: velnor_model::Host,
    })
    .key;
    let (name, id, word) = match verb {
        Verb::Restore => (
            "Restore the tools from the node store",
            Some(crate::gate::TOOLS_LOCAL_STEP),
            "restore",
        ),
        Verb::Save => ("Save the tools to the node store", None, "save"),
        Verb::SaveChecked => (
            "Save the tools to the node store",
            Some(crate::gate::TOOLS_LOCAL_SAVE_STEP),
            "save",
        ),
        Verb::Prime => ("Prime the node store with the tools", None, "save"),
        Verb::RecordRestore => return tools_restored_step(key),
        Verb::RecordSave => return tools_saved_step(),
    };
    RenderedStep {
        id: id.map(str::to_owned),
        name: name.to_owned(),
        continue_on_error: matches!(verb, Verb::SaveChecked | Verb::Prime),
        env: vec![(
            "VELNOR_TOOLS_KEY".to_owned(),
            crate::rules::Value::Expr(key),
        )],
        run: Some(format!(
            "velnor-ci tools-local {word} --key \"$VELNOR_TOOLS_KEY\""
        )),
        ..RenderedStep::default()
    }
}

/// The cache telemetry reads one restore step per cache. With a node store the tools come from
/// the node store or from the cache action, so this step reports whichever one served them.
fn tools_restored_step(key: String) -> RenderedStep {
    use crate::gate::{TOOLS_LOCAL_STEP, TOOLS_STEP};
    use crate::rules::Value;
    let local = format!("steps.{TOOLS_LOCAL_STEP}.outputs.hit == 'true'");
    let action = |name: &str| Value::Expr(format!("${{{{ steps.{TOOLS_STEP}.outputs.{name} }}}}"));
    RenderedStep {
        id: Some(crate::gate::TOOLS_RESTORED_STEP.to_owned()),
        name: "Record where the tools came from".to_owned(),
        env: vec![
            ("CACHE_KEY".to_owned(), Value::Expr(key)),
            (
                "NODE_HIT".to_owned(),
                Value::Expr(format!("${{{{ {local} && 'true' || '' }}}}")),
            ),
            ("MATCHED".to_owned(), action("cache-matched-key")),
            (
                "EXACT".to_owned(),
                Value::Expr(format!(
                    "${{{{ {local} && 'true' || steps.{TOOLS_STEP}.outputs.cache-hit }}}}"
                )),
            ),
        ],
        run: Some(
            "printf 'cache-primary-key=%s\\ncache-matched-key=%s\\ncache-hit=%s\\n' \"$CACHE_KEY\" \"${NODE_HIT:+$CACHE_KEY}$MATCHED\" \"$EXACT\" >> \"$GITHUB_OUTPUT\""
                .to_owned(),
        ),
        ..RenderedStep::default()
    }
}

/// Fails, without failing the job, when either save failed, so the cache telemetry sees it.
/// It is skipped when neither save ran, so that the telemetry sees no save attempt.
fn tools_saved_step() -> RenderedStep {
    use crate::gate::{TOOLS_LOCAL_SAVE_STEP, TOOLS_SAVED_STEP};
    use crate::rules::Value;
    let network = crate::report::cache_id(
        &velnor_model::CacheKey::Tools {
            host: velnor_model::Host,
        },
        false,
    );
    let outcome = |id: &str| format!("steps.{id}.outcome");
    RenderedStep {
        id: Some(TOOLS_SAVED_STEP.to_owned()),
        name: "Record the tools save".to_owned(),
        condition: Some(format!(
            "${{{{ !cancelled() && ({} == 'failure' || {} == 'success' || {} == 'failure') }}}}",
            outcome(TOOLS_LOCAL_SAVE_STEP),
            outcome(&network),
            outcome(&network)
        )),
        continue_on_error: true,
        env: vec![(
            "CACHE_OK".to_owned(),
            Value::Expr(format!(
                "${{{{ {} != 'failure' && {} != 'failure' }}}}",
                outcome(TOOLS_LOCAL_SAVE_STEP),
                outcome(&network)
            )),
        )],
        run: Some("velnor-ci cache check --success \"$CACHE_OK\"".to_owned()),
        ..RenderedStep::default()
    }
}

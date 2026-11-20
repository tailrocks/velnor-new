// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CacheKey, VerifiedSave, When};

use crate::gate::{
    ON_DEFAULT_BRANCH, SAVING_EVENTS, TOOLS_LOCAL_STEP, TOOLS_LOOKUP_STEP, TOOLS_STEP,
};
use crate::rules::{RenderedStep, Value};

const VERIFY_STEP: &str = "toolsverify";
const RECORD_STEP: &str = "toolsrecord";
const ANCHOR_STEP: &str = "toolsanchor";
const ANCHOR_DIR: &str = "${{ runner.temp }}/velnor-tools-anchor";

/// On a node miss the lookup is skipped, so its empty output leaves the network restore in charge.
pub(crate) fn tools_parts(when: When, parts: &mut Vec<String>) {
    let on_default = ON_DEFAULT_BRANCH.to_owned();
    let saving = SAVING_EVENTS.to_owned();
    let node_miss = format!("steps.{TOOLS_LOCAL_STEP}.outputs.hit != 'true'");
    let network_miss = format!("steps.{TOOLS_STEP}.outputs.cache-hit != 'true'");
    let lookup_miss = format!("steps.{TOOLS_LOOKUP_STEP}.outputs.cache-hit != 'true'");
    let matches = format!("steps.{RECORD_STEP}.outputs.matches == 'true'");
    match when {
        When::PrimeToolsFromNetwork => parts.extend([
            on_default,
            saving,
            node_miss,
            format!("steps.{TOOLS_STEP}.outputs.cache-hit == 'true'"),
        ]),
        When::LookupToolsOnNodeHit => parts.extend([
            on_default,
            format!("steps.{TOOLS_LOCAL_STEP}.outputs.hit == 'true'"),
        ]),
        When::SaveToolsOnDefaultBranch => parts.extend([on_default, network_miss]),
        When::SaveToolsUnlessLocal => parts.extend([on_default, network_miss, lookup_miss]),
        When::RestoreToolsUnlessLocal => parts.push(node_miss),
        When::SaveVerifiedTools(save) => {
            let recorded = format!("steps.{RECORD_STEP}.outputs.recorded == 'true'");
            match save {
                VerifiedSave::OnDefaultBranch => parts.extend([on_default, network_miss, matches]),
                VerifiedSave::UnlessLocal => {
                    parts.extend([on_default, network_miss, lookup_miss, matches]);
                }
                VerifiedSave::NodeStore => parts.extend([on_default, matches]),
                VerifiedSave::Anchor => parts.extend([on_default, recorded]),
                VerifiedSave::Prime => {
                    tools_parts(When::PrimeToolsFromNetwork, parts);
                    parts.push(format!("steps.{VERIFY_STEP}.outputs.verified == 'true'"));
                }
            }
        }
        _ => {}
    }
}

fn anchor_cache_step(action: &str, verb: &str) -> RenderedStep {
    let key = crate::cache::render_key(&CacheKey::Tools {
        host: velnor_model::Host,
    })
    .key;
    let key = key.replacen("velnor-tools-", "velnor-tools-anchor-", 1);
    RenderedStep {
        id: (verb == "Restore").then(|| ANCHOR_STEP.to_owned()),
        name: format!("{verb} anchor"),
        shared: true,
        uses: Some(action.to_owned()),
        with: vec![
            ("path".to_owned(), Value::Expr(ANCHOR_DIR.to_owned())),
            ("key".to_owned(), Value::Expr(key)),
        ],
        ..RenderedStep::default()
    }
}

fn verify_step(verb: velnor_model::ToolsLocalVerb) -> RenderedStep {
    use velnor_model::ToolsLocalVerb as Verb;
    let (id, name, word) = match verb {
        Verb::AnchorReset => (None, "Clear anchor", "reset"),
        Verb::Verify => (Some(VERIFY_STEP), "Verify tools", "check"),
        _ => (Some(RECORD_STEP), "Record anchor", "record"),
    };
    let hit = Value::Expr(format!("${{{{ steps.{ANCHOR_STEP}.outputs.cache-hit }}}}"));
    let mut env: Vec<(String, Value)> = (verb != Verb::AnchorReset)
        .then(|| ("VELNOR_ANCHOR_HIT".to_owned(), hit))
        .into_iter()
        .collect();
    if verb == Verb::Verify {
        env.push((
            "VELNOR_TOOLS_CACHE_KEY".to_owned(),
            Value::Expr(format!(
                "${{{{ steps.{TOOLS_STEP}.outputs.cache-matched-key }}}}"
            )),
        ));
    }
    RenderedStep {
        id: id.map(str::to_owned),
        name: name.to_owned(),
        shared: true,
        env,
        run: Some(format!("velnor-ci tools-verify {word}")),
        ..RenderedStep::default()
    }
}

/// The exact-key lookup the node-hit path runs on a trusted run: `lookup-only`, so nothing is
/// downloaded, and no fallback keys, so `cache-hit` means the exact tools entry exists.
pub(crate) fn tools_lookup(key: &CacheKey) -> RenderedStep {
    let mut step = crate::cache::cache_step(crate::steps::CACHE_RESTORE, "Look up", key, false);
    step.id = Some(crate::gate::TOOLS_LOOKUP_STEP.to_owned());
    step.with
        .push(("lookup-only".to_owned(), Value::Text("true".to_owned())));
    step
}

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
        Verb::AnchorRestore => return anchor_cache_step(crate::steps::CACHE_RESTORE, "Restore"),
        Verb::AnchorSave => return anchor_cache_step(crate::steps::CACHE_SAVE, "Save"),
        Verb::AnchorReset | Verb::Verify | Verb::Record => return verify_step(verb),
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

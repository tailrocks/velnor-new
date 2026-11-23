// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    AnchorSource, CacheKey, TOOLS_ANCHOR_KEY_OUTPUT as ANCHOR_KEY_OUTPUT,
    TOOLS_ANCHOR_OUTPUT as ANCHOR_OUTPUT, ToolsAnchor, ToolsLocalVerb, VerifiedSave, When,
};

use crate::gate::{
    ON_DEFAULT_BRANCH, SAVING_EVENTS, TOOLS_LOCAL_STEP, TOOLS_LOOKUP_STEP, TOOLS_STEP,
};
use crate::rules::{RenderedStep, Value};
use crate::tools_anchor_steps::{
    ANCHOR_STEP, READ_STEP, anchor_cache_step, newest_anchor_step, read_step,
};

const VERIFY_STEP: &str = "toolsverify";
const RECORD_STEP: &str = "toolsrecord";

pub(crate) fn anchor_expression(anchor: &ToolsAnchor) -> String {
    match anchor {
        ToolsAnchor::Unanchored => String::new(),
        ToolsAnchor::Restored(AnchorSource::ThisJob) => {
            format!("${{{{ steps.{READ_STEP}.outputs.anchor }}}}")
        }
        ToolsAnchor::Restored(AnchorSource::Job(job)) => {
            format!("${{{{ needs.{job}.outputs.{ANCHOR_OUTPUT} }}}}")
        }
        ToolsAnchor::Recorded => format!("${{{{ steps.{RECORD_STEP}.outputs.digest }}}}"),
    }
}

pub(crate) fn is_anchor_expression(expression: &str) -> bool {
    [
        ToolsAnchor::Restored(AnchorSource::ThisJob),
        ToolsAnchor::Recorded,
    ]
    .iter()
    .any(|anchor| anchor_expression(anchor) == format!("${{{{ {expression} }}}}"))
        || expression.ends_with(&format!(".outputs.{ANCHOR_OUTPUT}"))
}

pub(crate) fn anchor_outputs(job: &velnor_model::Job) -> Vec<(String, String)> {
    let reads = job.steps.iter().any(|step| {
        matches!(
            step.kind,
            velnor_model::StepKind::ToolsLocal(ToolsLocalVerb::AnchorRead, _)
        )
    });
    if !reads {
        return Vec::new();
    }
    let mut outputs = vec![(
        ANCHOR_OUTPUT.to_owned(),
        anchor_expression(&ToolsAnchor::Restored(AnchorSource::ThisJob)),
    )];
    if job.anchor_key_output {
        outputs.push((
            ANCHOR_KEY_OUTPUT.to_owned(),
            format!("${{{{ steps.{ANCHOR_STEP}.outputs.cache-matched-key }}}}"),
        ));
    }
    outputs
}

fn anchor_key_expression(anchor: &ToolsAnchor) -> String {
    match anchor {
        ToolsAnchor::Restored(AnchorSource::Job(job)) => {
            format!("${{{{ needs.{job}.outputs.{ANCHOR_KEY_OUTPUT} }}}}")
        }
        _ => format!("${{{{ steps.{ANCHOR_STEP}.outputs.cache-matched-key }}}}"),
    }
}

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
                VerifiedSave::AnchoredOnDefaultBranch => {
                    parts.extend([
                        on_default,
                        matches,
                        format!("({recorded} || {network_miss})"),
                    ]);
                }
                VerifiedSave::AnchoredUnlessLocal => parts.extend([
                    on_default,
                    matches,
                    format!("({recorded} || ({network_miss} && {lookup_miss}))"),
                ]),
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

fn verify_step(verb: ToolsLocalVerb, anchor: &ToolsAnchor) -> RenderedStep {
    let (id, name, word) = match verb {
        ToolsLocalVerb::AnchorReset => (None, "Clear anchor", "reset"),
        ToolsLocalVerb::Verify
        | ToolsLocalVerb::VerifyNamed
        | ToolsLocalVerb::VerifyNamedAnchor => (Some(VERIFY_STEP), "Verify tools", "check"),
        _ => (Some(RECORD_STEP), "Record anchor", "record"),
    };
    let mut env = if *anchor == ToolsAnchor::Unanchored {
        let hit = Value::Expr(format!("${{{{ steps.{ANCHOR_STEP}.outputs.cache-hit }}}}"));
        (verb != ToolsLocalVerb::AnchorReset)
            .then(|| ("VELNOR_ANCHOR_HIT".to_owned(), hit))
            .into_iter()
            .collect()
    } else {
        let mut env = vec![(
            "VELNOR_TOOLS_ANCHOR".to_owned(),
            Value::Expr(anchor_expression(anchor)),
        )];
        if verb == ToolsLocalVerb::Record {
            env.push((
                "VELNOR_TOOLS_VERIFIED".to_owned(),
                Value::Expr(format!("${{{{ steps.{VERIFY_STEP}.outputs.verified }}}}")),
            ));
        }
        env
    };
    if verb == ToolsLocalVerb::VerifyNamedAnchor {
        env.push((
            "VELNOR_ANCHOR_CACHE_KEY".to_owned(),
            Value::Expr(anchor_key_expression(anchor)),
        ));
    }
    if matches!(
        verb,
        ToolsLocalVerb::VerifyNamed | ToolsLocalVerb::VerifyNamedAnchor
    ) {
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

fn clear_step() -> RenderedStep {
    RenderedStep {
        name: "Clear the tools directories".to_owned(),
        run: Some("velnor-ci tools-local clear".to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn tools_local_step(verb: ToolsLocalVerb, anchor: &ToolsAnchor) -> RenderedStep {
    use velnor_model::ToolsLocalVerb as Verb;
    let key = crate::cache::render_key(&velnor_model::CacheKey::Tools {
        host: velnor_model::Host,
        anchor: anchor.clone(),
    })
    .key;
    let (name, id, word) = match verb {
        Verb::Clear => return clear_step(),
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
        Verb::RecordSavedKey => return tools_saved_key_step(key),
        Verb::AnchorRestore => return anchor_cache_step(crate::steps::CACHE_RESTORE, "Restore"),
        Verb::AnchorSave if *anchor == ToolsAnchor::Recorded => {
            return newest_anchor_step(crate::steps::CACHE_SAVE, false, false);
        }
        Verb::AnchorSaveAttempt if *anchor == ToolsAnchor::Recorded => {
            return newest_anchor_step(crate::steps::CACHE_SAVE, false, true);
        }
        Verb::AnchorSave | Verb::AnchorSaveAttempt => {
            return anchor_cache_step(crate::steps::CACHE_SAVE, "Save");
        }
        Verb::AnchorFind => return newest_anchor_step(crate::steps::CACHE_RESTORE, true, false),
        Verb::AnchorFindAttempt => {
            return newest_anchor_step(crate::steps::CACHE_RESTORE, true, true);
        }
        Verb::AnchorRead => return read_step(),
        Verb::AnchorReset
        | Verb::Verify
        | Verb::VerifyNamed
        | Verb::VerifyNamedAnchor
        | Verb::Record => {
            return verify_step(verb, anchor);
        }
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

fn tools_saved_key_step(key: String) -> RenderedStep {
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
        condition: Some(format!(
            "${{{{ !cancelled() && steps.{network}.outcome != 'skipped' }}}}"
        )),
        env: vec![("CACHE_KEY".to_owned(), Value::Expr(key))],
        run: Some("echo \"key=$CACHE_KEY\" >> \"$GITHUB_OUTPUT\"".to_owned()),
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
            anchor: ToolsAnchor::Unanchored,
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

#[cfg(test)]
mod tests {
    use velnor_model::{AnchorSource, ToolsAnchor, ToolsLocalVerb};

    use super::tools_local_step;

    #[test]
    fn an_unrecorded_anchor_saves_by_the_fixed_key_whichever_save_verb_asks() {
        for anchor in [
            ToolsAnchor::Unanchored,
            ToolsAnchor::Restored(AnchorSource::ThisJob),
        ] {
            let fixed = tools_local_step(ToolsLocalVerb::AnchorSave, &anchor);
            assert_eq!(
                tools_local_step(ToolsLocalVerb::AnchorSaveAttempt, &anchor),
                fixed
            );
        }
    }

    fn save_key(step: &crate::rules::RenderedStep) -> String {
        step.with
            .iter()
            .find_map(|(name, value)| match (name.as_str(), value) {
                ("key", crate::rules::Value::Expr(key)) => Some(key.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn a_recorded_anchor_saves_by_the_run_key_or_the_run_attempt_key_as_asked() {
        let run = tools_local_step(ToolsLocalVerb::AnchorSave, &ToolsAnchor::Recorded);
        let attempt = tools_local_step(ToolsLocalVerb::AnchorSaveAttempt, &ToolsAnchor::Recorded);
        assert!(save_key(&run).ends_with("-${{ github.run_id }}-${{ github.job }}"));
        assert!(
            save_key(&attempt)
                .ends_with("-${{ github.run_id }}-${{ github.run_attempt }}-${{ github.job }}")
        );
    }
}

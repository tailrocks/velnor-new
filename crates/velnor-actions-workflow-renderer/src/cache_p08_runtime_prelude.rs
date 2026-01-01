//! Fixed hosted composite for V2 runtime identity and exact-key seed import.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_PRELUDE_USES;
use velnor_actions_contract::{Step, StepId, StepKind, StepRole};

use crate::{
    RenderError, cache_p08, marker,
    tree::RenderedFile,
    yaml::{Yaml, render_yaml},
};

const ACTIONS: [(&str, &str); 3] = [
    ("./.github/actions/u22", TOOLS_CACHE_PRELUDE_USES[0]),
    ("./.github/actions/u24", TOOLS_CACHE_PRELUDE_USES[1]),
    ("./.github/actions/u26", TOOLS_CACHE_PRELUDE_USES[2]),
];

pub(super) fn action_uses(runs_on: &str) -> Option<&'static str> {
    let identity = super::runtime_identity::action_uses(runs_on)?;
    ACTIONS
        .iter()
        .find_map(|(candidate, prelude)| (*candidate == identity).then_some(*prelude))
}

pub(super) fn step(payload: &super::ToolsCachePayload) -> Result<Step, RenderError> {
    let uses = action_uses(&payload.runs_on).ok_or_else(|| {
        RenderError::BadCommand("unsupported_tools_cache_identity_lane".to_owned())
    })?;
    Ok(Step {
        name: cache_p08::TOOLS_CACHE_IDENTITY_NAME.to_owned(),
        id: Some(StepId::ToolsCacheIdentity),
        role: Some(StepRole::ToolsCacheIdentity),
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::from([(
                cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
                payload.static_digest.clone(),
            )]),
            env: BTreeMap::new(),
        },
    })
}

pub(super) fn validate_action_call(
    step: &Step,
    uses: &str,
    runs_on: &str,
    with: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if step.role != Some(StepRole::ToolsCacheIdentity)
        || Some(uses) != action_uses(runs_on)
        || !env.is_empty()
        || with.len() != 1
        || !with.contains_key(cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
    {
        return Err(RenderError::InvalidWorkflow(
            "malformed_tools_cache_identity_action".to_owned(),
        ));
    }
    let digest = with
        .get(cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("malformed_tools_cache_identity_action".to_owned())
        })?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(RenderError::InvalidWorkflow(
            "bad_tools_cache_identity_digest".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn action_file(runs_on: &str, version: &str) -> Result<RenderedFile, RenderError> {
    let uses = action_uses(runs_on).ok_or_else(|| {
        RenderError::BadCommand("unsupported_tools_cache_identity_lane".to_owned())
    })?;
    let inner_identity = identity_step(runs_on)?;
    let inner_seed = seed_step();
    let outputs = output_map();
    let body = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Velnor hosted tools prelude")),
        (
            "description".to_owned(),
            Yaml::str("Qualify the hosted image and copy a matching tool seed."),
        ),
        ("inputs".to_owned(), input_map()),
        ("outputs".to_owned(), outputs),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite")),
                (
                    "steps".to_owned(),
                    Yaml::Seq(vec![inner_identity, inner_seed]),
                ),
            ]),
        ),
    ]);
    let bytes = marker::with_marker(version, &render_yaml(&body))?;
    crate::steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!("{}/action.yml", uses.trim_start_matches("./")),
        bytes,
    })
}

fn identity_step(runs_on: &str) -> Result<Yaml, RenderError> {
    let uses = super::runtime_identity::action_uses(runs_on).ok_or_else(|| {
        RenderError::BadCommand("unsupported_tools_cache_identity_lane".to_owned())
    })?;
    let entries = vec![
        (
            "name".to_owned(),
            Yaml::str(cache_p08::TOOLS_CACHE_IDENTITY_NAME),
        ),
        (
            "id".to_owned(),
            Yaml::str(cache_p08::TOOLS_CACHE_IDENTITY_STEP_ID),
        ),
        (
            "uses".to_owned(),
            Yaml::annotated(uses, "zizmor: ignore[self-repository]"),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![(
                cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
                Yaml::str("${{ inputs.d }}"),
            )]),
        ),
    ];
    Ok(Yaml::Map(entries))
}

fn seed_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str(crate::tool_seed::TOOL_SEED_NAME),
        ),
        ("id".to_owned(), Yaml::str("seed")),
        (
            "if".to_owned(),
            Yaml::str(cache_p08::TOOLS_CACHE_RESTORE_CONDITION),
        ),
        (
            "uses".to_owned(),
            Yaml::annotated(
                crate::tool_seed::TOOL_SEED_USES,
                "zizmor: ignore[self-repository]",
            ),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![(
                "cache_key".to_owned(),
                Yaml::str(cache_p08::TOOLS_CACHE_KEY_EXPRESSION),
            )]),
        ),
    ])
}

fn input_map() -> Yaml {
    Yaml::Map(vec![(
        cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
        Yaml::Map(vec![
            (
                "description".to_owned(),
                Yaml::str("Static V2 identity of the tool pins and owned paths."),
            ),
            ("required".to_owned(), Yaml::Bool(true)),
        ]),
    )])
}

fn output_map() -> Yaml {
    let output = |key: &str| {
        Yaml::Map(vec![(
            "value".to_owned(),
            Yaml::str(format!("${{{{ steps.v2.outputs.{key} }}}}")),
        )])
    };
    Yaml::Map(vec![
        ("enabled".to_owned(), output("enabled")),
        ("identity".to_owned(), output("identity")),
        (
            "seed_admitted".to_owned(),
            Yaml::Map(vec![(
                "value".to_owned(),
                Yaml::str("${{ steps.seed.outputs.seed_admitted }}"),
            )]),
        ),
    ])
}

#[cfg(test)]
#[path = "cache_p08_runtime_prelude_tests.rs"]
mod tests;

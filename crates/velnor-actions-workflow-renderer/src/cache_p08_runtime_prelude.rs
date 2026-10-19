//! Fixed composite for V2 runner identity and exact-key seed import.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_PRELUDE_USES;
use velnor_actions_contract::{RunsOn, Step, StepId, StepKind, StepRole};

use crate::{
    RenderError, cache_p08, marker,
    tree::RenderedFile,
    yaml::{Yaml, render_yaml},
};

const ACTIONS: [(&str, &str); 4] = [
    ("./.github/actions/u22", TOOLS_CACHE_PRELUDE_USES[0]),
    ("./.github/actions/u24", TOOLS_CACHE_PRELUDE_USES[1]),
    ("./.github/actions/u26", TOOLS_CACHE_PRELUDE_USES[2]),
    ("./.github/actions/scale-set", TOOLS_CACHE_PRELUDE_USES[3]),
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
    let mut with = BTreeMap::from([(
        cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
        payload.identity_input(),
    )]);
    if let Some(source_sha) = &payload.scale_set_image_source_sha {
        with.insert(
            super::runtime_identity::TOOLS_CACHE_SCALE_SET_IMAGE_SOURCE_SHA_INPUT.to_owned(),
            source_sha.clone(),
        );
    }
    let is_scale_set = is_scale_set_lane(&payload.runs_on)?;
    if is_scale_set && payload.scale_set_image_source_sha.is_none() {
        return Err(RenderError::BadCommand(
            "tools_cache_scale_set_image_source_missing".to_owned(),
        ));
    }
    Ok(Step {
        parallel: false,
        name: cache_p08::TOOLS_CACHE_IDENTITY_NAME.to_owned(),
        id: Some(StepId::ToolsCacheIdentity),
        role: Some(StepRole::ToolsCacheIdentity),
        condition: Some(cache_p08::TOOLS_CACHE_NORMAL_RUN_CONDITION.to_owned()),
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
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
    let is_scale_set = is_scale_set_lane(runs_on)?;
    let source_sha =
        with.get(super::runtime_identity::TOOLS_CACHE_SCALE_SET_IMAGE_SOURCE_SHA_INPUT);
    if step.role != Some(StepRole::ToolsCacheIdentity)
        || !matches!(
            step.condition.as_deref(),
            Some(
                cache_p08::TOOLS_CACHE_NORMAL_RUN_CONDITION
                    | cache_p08::TOOLS_CACHE_PLAN_RUN_CONDITION
            )
        )
        || Some(uses) != action_uses(runs_on)
        || !env.is_empty()
        || with.len() != if is_scale_set { 2 } else { 1 }
        || !with.contains_key(cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
        || (is_scale_set && !source_sha.is_some_and(|value| is_image_source_sha(value)))
        || (!is_scale_set && source_sha.is_some())
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
    if !velnor_actions_contract::config::project_tool_refs::is_tools_cache_identity_input(digest) {
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
        ("name".to_owned(), Yaml::str("Velnor tools prelude")),
        (
            "description".to_owned(),
            Yaml::str("Check the runner image and copy a matching tool seed."),
        ),
        ("inputs".to_owned(), input_map(runs_on)?),
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
    let is_scale_set = is_scale_set_lane(runs_on)?;
    let mut with = vec![(
        cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
        Yaml::str("${{ inputs.d }}"),
    )];
    if is_scale_set {
        with.push((
            super::runtime_identity::TOOLS_CACHE_SCALE_SET_IMAGE_SOURCE_SHA_INPUT.to_owned(),
            Yaml::str("${{ inputs.scale-set-image-source-sha }}"),
        ));
    }
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
        ("with".to_owned(), Yaml::Map(with)),
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
            Yaml::str(cache_p08::TOOLS_CACHE_IDENTITY_CONDITION),
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

fn input_map(runs_on: &str) -> Result<Yaml, RenderError> {
    let mut inputs = vec![(
        cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
        Yaml::Map(vec![
            (
                "description".to_owned(),
                Yaml::str("Static V2 identity of the tool pins and owned paths."),
            ),
            ("required".to_owned(), Yaml::Bool(true)),
        ]),
    )];
    if is_scale_set_lane(runs_on)? {
        inputs.push((
            super::runtime_identity::TOOLS_CACHE_SCALE_SET_IMAGE_SOURCE_SHA_INPUT.to_owned(),
            Yaml::Map(vec![
                (
                    "description".to_owned(),
                    Yaml::str("Validated immutable runner-image source SHA."),
                ),
                ("required".to_owned(), Yaml::Bool(true)),
            ]),
        ));
    }
    Ok(Yaml::Map(inputs))
}

fn is_scale_set_lane(runs_on: &str) -> Result<bool, RenderError> {
    RunsOn::parse(runs_on)
        .map(|runs_on| runs_on.is_scale_set())
        .map_err(|error| RenderError::BadCommand(format!("bad_tools_cache_lane:{error}")))
}

fn is_image_source_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

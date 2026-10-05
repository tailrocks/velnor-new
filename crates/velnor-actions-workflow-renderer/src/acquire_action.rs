//! Immutable, source-bound local actions for digest-verified acquisition.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole, canonical_json_bytes};

use crate::{RenderError, composite::composite_yaml, render::RenderContext, tree::RenderedFile};

const ACTION_NAME_PREFIX: &str = "acquire-b3-";
const PROVENANCE_KEYS: [&str; 3] = [
    crate::steps::ASSET_SHA_ENV,
    crate::steps::ASSET_URL_ENV,
    crate::steps::RELEASE_COMMIT_ENV,
];

/// One generated action per unique canonical typed acquisition source.
#[derive(Default)]
pub(crate) struct AcquireActions {
    files: BTreeMap<String, RenderedFile>,
}

impl AcquireActions {
    /// Replace every acquisition step in a serialized lane section.
    ///
    /// # Errors
    pub(crate) fn transform_steps(
        &mut self,
        steps: &mut [Step],
        ctx: &RenderContext,
    ) -> Result<bool, RenderError> {
        let mut replaced = false;
        for step in steps {
            if step.role == Some(StepRole::AcquireVelnor) {
                *step = self.call_step(step, ctx)?;
                replaced = true;
            }
        }
        Ok(replaced)
    }

    /// Replace a validated Acquire step with a fixed local action call.
    ///
    /// The action identity hashes the complete typed step, including its
    /// literal release tuple and fixed command vector. Its caller has no
    /// `with` or `env` inputs, so workflow environment mutations cannot
    /// replace the source tuple.
    /// # Errors
    pub(crate) fn call_step(
        &mut self,
        source: &Step,
        ctx: &RenderContext,
    ) -> Result<Step, RenderError> {
        validate_source(source)?;
        let identity = canonical_json_bytes(source).map_err(RenderError::Contract)?;
        let digest = velnor_actions_contract::digest_b3(&identity);
        let digest = digest
            .strip_prefix("b3-")
            .filter(|value| velnor_actions_contract::ids::is_lower_hex_len(value, 64))
            .ok_or_else(|| RenderError::InvalidWorkflow("acquire_digest_invalid".to_owned()))?;
        let name = format!("{ACTION_NAME_PREFIX}{digest}");
        let uses = format!(
            "{}{digest}",
            velnor_actions_contract::workflow::step_identity::ACQUIRE_VELNOR_ACTION_PREFIX
        );
        let candidate = action_file(&name, source, ctx)?;
        if let Some(existing) = self.files.get(&name) {
            if existing.bytes != candidate.bytes {
                return Err(RenderError::InvalidWorkflow(
                    "acquire_digest_collision".to_owned(),
                ));
            }
        } else {
            self.files.insert(name, candidate);
        }
        Ok(Step {
            name: source.name.clone(),
            id: source.id,
            role: Some(StepRole::AcquireVelnor),
            condition: source.condition.clone(),
            kind: StepKind::Action {
                uses,
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        })
    }

    /// Append generated acquire action files in stable path order.
    pub(crate) fn append_files(self, files: &mut Vec<RenderedFile>) {
        files.extend(self.files.into_values());
    }
}

/// Whether an action ref is an exact generated acquisition action path.
#[must_use]
pub(crate) fn is_acquire_action_uses(uses: &str) -> bool {
    velnor_actions_contract::workflow::step_identity::is_acquire_action_uses(uses)
}

/// Whether a key belongs to the typed asset/source tuple.
#[must_use]
pub(crate) fn is_provenance_key(key: &str) -> bool {
    PROVENANCE_KEYS.contains(&key)
}

/// Validate the exact typed source tuple and shell payload before materializing it.
fn validate_source(step: &Step) -> Result<(), RenderError> {
    if step.role != Some(StepRole::AcquireVelnor) {
        return Err(RenderError::InvalidWorkflow(
            "acquire_role_missing".to_owned(),
        ));
    }
    let StepKind::Shell { run, env } = &step.kind else {
        return Err(RenderError::InvalidWorkflow("acquire_malformed".to_owned()));
    };
    let provenance = PROVENANCE_KEYS
        .iter()
        .map(|key| {
            env.get(*key)
                .map(|value| ((*key).to_owned(), value.clone()))
                .ok_or_else(|| RenderError::InvalidWorkflow("acquire_malformed".to_owned()))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let expected = crate::steps::acquire_velnor_step(run.clone(), &provenance)?;
    let StepKind::Shell {
        env: expected_env, ..
    } = expected.kind
    else {
        return Err(RenderError::InvalidWorkflow("acquire_malformed".to_owned()));
    };
    if &expected_env != env {
        return Err(RenderError::InvalidWorkflow("acquire_malformed".to_owned()));
    }
    Ok(())
}

/// One immutable local composite action containing the original shell step.
fn action_file(
    name: &str,
    source: &Step,
    ctx: &RenderContext,
) -> Result<RenderedFile, RenderError> {
    let step =
        crate::document_steps::step_to_yaml(name, source, ctx, &[], true, &BTreeMap::new(), false)?;
    let body = composite_yaml(name, vec![step])?;
    let quoted = crate::yaml::quote_run_values_in_yaml(body);
    let bytes =
        crate::marker::with_marker(&ctx.generator_version, &crate::yaml::render_yaml(&quoted))?;
    crate::steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{name}/action.yml"),
        bytes,
    })
}

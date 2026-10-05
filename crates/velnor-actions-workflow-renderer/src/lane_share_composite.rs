use velnor_actions_contract::workflow::Step;

use crate::composite::composite_yaml;
use crate::document_steps::step_to_yaml;
use crate::render::RenderContext;
use crate::tree::RenderedFile;
use crate::{RenderError, marker, steps, yaml::render_yaml};

pub(crate) fn composite_file(
    logical: &str,
    steps: &[Step],
    ctx: &RenderContext,
    acquire_actions: &mut crate::acquire_action::AcquireActions,
) -> Result<RenderedFile, RenderError> {
    velnor_actions_contract::workflow::step_identity::validate_step_identity_scope(
        steps,
        &format!("composite:{logical}"),
    )
    .map_err(RenderError::Contract)?;
    let mut rendered = Vec::with_capacity(steps.len());
    let mut serialized_steps = Vec::with_capacity(steps.len());
    for step in steps {
        let serialized = if step.role == Some(velnor_actions_contract::StepRole::AcquireVelnor) {
            acquire_actions.call_step(step, ctx)?
        } else {
            step.clone()
        };
        rendered.push(step_to_yaml(
            logical,
            &serialized,
            ctx,
            &[],
            true,
            &std::collections::BTreeMap::new(),
            false,
        )?);
        serialized_steps.push(serialized);
    }
    velnor_actions_contract::workflow::step_identity::validate_step_identity_scope(
        &serialized_steps,
        &format!("composite:{logical}"),
    )
    .map_err(RenderError::Contract)?;
    let body = composite_yaml(logical, rendered)?;
    let quoted = crate::yaml::quote_run_values_in_yaml(body);
    let bytes = marker::with_marker(&ctx.generator_version, &render_yaml(&quoted))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!(".github/actions/{logical}/action.yml"),
        bytes,
    })
}

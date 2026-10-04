//! Every Plan exports its actual typed publication fallback result.

use crate::{RenderError, render::PLAN_JOB_ID, yaml::Yaml};
use velnor_actions_contract::PLAN_CARGO_FALLBACK_OUTPUT;

/// Bind the output to the validated Plan publication, including ordinary `false`.
pub(crate) fn attach(document: &mut Yaml) -> Result<(), RenderError> {
    let outputs = plan_outputs(document)
        .ok_or_else(|| RenderError::InvalidWorkflow("fallback_without_plan_outputs".to_owned()))?;
    if outputs
        .iter()
        .any(|(key, _)| key == PLAN_CARGO_FALLBACK_OUTPUT)
    {
        return Err(RenderError::InvalidWorkflow(
            "duplicate_plan_fallback_output".to_owned(),
        ));
    }
    outputs.push((
        PLAN_CARGO_FALLBACK_OUTPUT.to_owned(),
        Yaml::str(format!(
            "${{{{ steps.plan.outputs.{PLAN_CARGO_FALLBACK_OUTPUT} }}}}"
        )),
    ));
    Ok(())
}

fn plan_outputs(document: &mut Yaml) -> Option<&mut Vec<(String, Yaml)>> {
    let Yaml::Map(entries) = document else {
        return None;
    };
    let Yaml::Map(jobs) = &mut entries.iter_mut().find(|(key, _)| key == "jobs")?.1 else {
        return None;
    };
    let Yaml::Map(plan) = &mut jobs.iter_mut().find(|(key, _)| key == PLAN_JOB_ID)?.1 else {
        return None;
    };
    let Yaml::Map(outputs) = &mut plan.iter_mut().find(|(key, _)| key == "outputs")?.1 else {
        return None;
    };
    Some(outputs)
}

#[cfg(test)]
mod tests {
    use super::attach;
    use crate::yaml::Yaml;

    #[test]
    fn fallback_output_requires_existing_plan_outputs_and_cannot_overwrite() {
        let mut absent = Yaml::Map(Vec::new());
        assert!(attach(&mut absent).is_err());
        let mut document = Yaml::Map(vec![(
            "jobs".to_owned(),
            Yaml::Map(vec![(
                "plan".to_owned(),
                Yaml::Map(vec![(
                    "outputs".to_owned(),
                    Yaml::Map(vec![(
                        "covered_tasks".to_owned(),
                        Yaml::str("${{ steps.plan.outputs.covered_tasks }}"),
                    )]),
                )]),
            )]),
        )]);
        assert!(attach(&mut document).is_ok());
        assert!(attach(&mut document).is_err());
        let text = crate::yaml::render_yaml(&document);
        assert!(text.contains("covered_tasks: ${{ steps.plan.outputs.covered_tasks }}"));
        assert!(text.contains(
            "cargo_fallback_required: ${{ steps.plan.outputs.cargo_fallback_required }}"
        ));
    }
}

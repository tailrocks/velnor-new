use velnor_actions_contract::Step;

use crate::RenderError;
use crate::yaml::Yaml;

pub const MAX_CONCURRENT_BACKGROUND_STEPS: usize = 10;

pub const PARALLEL_JOIN_NAME: &str = "Wait for parallel checks";

pub(crate) fn render_step_sequence(
    steps: &[Step],
    mut render: impl FnMut(&Step) -> Result<Yaml, RenderError>,
) -> Result<Vec<Yaml>, RenderError> {
    let mut rendered = Vec::with_capacity(steps.len() + 1);
    let mut open_group = false;
    for step in steps {
        if open_group && !step.parallel {
            rendered.push(join_step());
            open_group = false;
        }
        let yaml = render(step)?;
        if step.parallel {
            rendered.push(background(yaml)?);
            open_group = true;
        } else {
            rendered.push(yaml);
        }
    }
    if open_group {
        rendered.push(join_step());
    }
    Ok(rendered)
}

pub(crate) fn reject_parallel(step: &Step, scope: &str) -> Result<(), RenderError> {
    if step.parallel {
        return Err(RenderError::InvalidWorkflow(format!(
            "parallel_step_outside_job:{scope}"
        )));
    }
    Ok(())
}

fn join_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(PARALLEL_JOIN_NAME.to_owned())),
        ("wait-all".to_owned(), Yaml::Null),
    ])
}

fn background(yaml: Yaml) -> Result<Yaml, RenderError> {
    let Yaml::Map(entries) = yaml else {
        return Err(RenderError::InvalidWorkflow(
            "parallel_step_not_mapping".to_owned(),
        ));
    };
    if entries
        .iter()
        .any(|(key, _)| key == "background" || key == "id")
    {
        return Err(RenderError::InvalidWorkflow(
            "parallel_step_authority".to_owned(),
        ));
    }
    let mut marked = Vec::with_capacity(entries.len() + 1);
    let mut inserted = false;
    for (key, value) in entries {
        let is_name = key == "name";
        marked.push((key, value));
        if is_name && !inserted {
            marked.push(("background".to_owned(), Yaml::Bool(true)));
            inserted = true;
        }
    }
    if !inserted {
        marked.insert(0, ("background".to_owned(), Yaml::Bool(true)));
    }
    Ok(Yaml::Map(marked))
}

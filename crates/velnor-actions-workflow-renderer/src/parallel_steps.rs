//! Native GitHub Actions step concurrency for one workflow job.
//!
//! A contiguous run of [`Step::parallel`] members forms one group. Each member
//! renders as a `background: true` step and the group ends with one
//! `wait-all` join before the next sequential step (or the end of the job).
//! GitHub fails the job at that join when any member failed, and a member's
//! outputs and environment changes become visible only after it, so members
//! carry no ids and no typed roles. Composite actions cannot declare
//! background steps, so composite rendering rejects parallel members.
//!
//! The emitted shape is the `background`/`wait-all` spelling rather than the
//! `parallel:` group sugar because the pinned actionlint still validates each
//! member's keys, expressions and scripts in that spelling; see
//! [`velnor_actions_actionlint::STEP_PARALLELISM_IGNORES`].

use velnor_actions_contract::Step;

use crate::RenderError;
use crate::yaml::Yaml;

/// GitHub runs at most ten background steps of one job at a time and queues
/// the rest until a slot is free.
pub const MAX_CONCURRENT_BACKGROUND_STEPS: usize = 10;

/// Display name of the generated join step after each parallel group.
pub const PARALLEL_JOIN_NAME: &str = "Wait for parallel checks";

/// Render a job's steps, emitting each contiguous parallel group as
/// background members followed by one `wait-all` join.
///
/// # Errors
///
/// Returns the first member render error, or rejects a member that does not
/// render to a step mapping or already carries a `background` key.
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

/// Reject parallel members where background steps are not supported.
///
/// # Errors
///
/// Returns [`RenderError::InvalidWorkflow`] for a parallel member in a
/// composite action or another non-job step list.
pub(crate) fn reject_parallel(step: &Step, scope: &str) -> Result<(), RenderError> {
    if step.parallel {
        return Err(RenderError::InvalidWorkflow(format!(
            "parallel_step_outside_job:{scope}"
        )));
    }
    Ok(())
}

/// The generated `wait-all` join; it always runs and takes no condition.
fn join_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(PARALLEL_JOIN_NAME.to_owned())),
        ("wait-all".to_owned(), Yaml::Null),
    ])
}

/// Mark one rendered member as a background step, right after its name.
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{Step, StepKind};

    use super::*;

    fn step(name: &str, parallel: bool) -> Step {
        Step {
            parallel,
            name: name.to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        }
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "matches the fallible per-step renderer signature"
    )]
    fn render(step: &Step) -> Result<Yaml, RenderError> {
        Ok(Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(step.name.clone())),
            ("run".to_owned(), Yaml::str("true".to_owned())),
        ]))
    }

    fn names(rendered: &[Yaml]) -> Vec<String> {
        rendered
            .iter()
            .map(|yaml| {
                let Yaml::Map(entries) = yaml else {
                    return String::new();
                };
                let name = entries
                    .iter()
                    .find(|(key, _)| key == "name")
                    .map(|(_, value)| crate::yaml::render_yaml(value))
                    .unwrap_or_default();
                let bg = entries.iter().any(|(key, _)| key == "background");
                let wait = entries.iter().any(|(key, _)| key == "wait-all");
                format!(
                    "{}{}{}",
                    name.trim(),
                    if bg { "+bg" } else { "" },
                    if wait { "+join" } else { "" }
                )
            })
            .collect()
    }

    #[test]
    fn each_contiguous_group_is_joined_before_the_next_sequential_step() {
        let steps = [
            step("Setup", false),
            step("Build", false),
            step("Clippy", true),
            step("Docs", true),
            step("Upload", false),
        ];
        let rendered = render_step_sequence(&steps, render).expect("renders");
        assert_eq!(
            names(&rendered),
            vec![
                "Setup",
                "Build",
                "Clippy+bg",
                "Docs+bg",
                "Wait for parallel checks+join",
                "Upload"
            ]
        );
        let yaml = crate::yaml::render_yaml(&Yaml::Seq(rendered));
        assert!(yaml.contains("- name: Wait for parallel checks\n  wait-all:\n"));
        assert!(yaml.contains("- name: Clippy\n  background: true\n"));
    }

    #[test]
    fn trailing_group_is_joined_and_sequential_jobs_are_unchanged() {
        let trailing = [step("A", false), step("B", true)];
        assert_eq!(
            names(&render_step_sequence(&trailing, render).expect("renders")),
            vec!["A", "B+bg", "Wait for parallel checks+join"]
        );
        let sequential = [step("A", false), step("B", false)];
        assert_eq!(
            names(&render_step_sequence(&sequential, render).expect("renders")),
            vec!["A", "B"]
        );
    }

    #[test]
    fn composite_scopes_reject_parallel_members() {
        assert!(reject_parallel(&step("A", false), "composite").is_ok());
        assert!(
            reject_parallel(&step("A", true), "composite")
                .is_err_and(|err| err.to_string().contains("parallel_step_outside_job"))
        );
    }
}

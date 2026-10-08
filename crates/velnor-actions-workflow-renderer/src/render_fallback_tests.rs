use super::{render_checked_workflow, render_marked_workflow};
use crate::{
    RenderError, marker,
    workflow_size::MAX_WORKFLOW_BYTES,
    yaml::{Yaml, render_yaml},
};

const VERSION: &str = "0.1.4";

fn workflow(command: &str, repeats: usize) -> Yaml {
    let steps = (0..repeats)
        .map(|index| {
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(format!("step-{index}"))),
                ("run".to_owned(), Yaml::str(command)),
            ])
        })
        .collect();
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "job".to_owned(),
            Yaml::Map(vec![("steps".to_owned(), Yaml::Seq(steps))]),
        )]),
    )])
}

#[test]
fn under_cap_workflow_is_byte_identical_even_when_sharing_is_profitable() -> Result<(), RenderError>
{
    let command = format!("echo {}", "x".repeat(8_000));
    let document = workflow(&command, 2);
    let canonical = marker::with_marker(VERSION, &render_yaml(&document))?;
    assert!(canonical.len() < MAX_WORKFLOW_BYTES);
    assert_eq!(
        render_marked_workflow(".github/workflows/ci.yml", &document, VERSION)?,
        canonical
    );
    assert_eq!(
        render_checked_workflow(".github/workflows/ci.yml", &document, VERSION)?,
        canonical
    );
    Ok(())
}

#[test]
fn non_workflow_action_document_keeps_canonical_bytes() -> Result<(), RenderError> {
    let document = Yaml::Map(vec![(
        "runs".to_owned(),
        Yaml::Map(vec![
            ("using".to_owned(), Yaml::str("composite")),
            (
                "steps".to_owned(),
                Yaml::Seq(
                    (0..3)
                        .map(|_| {
                            Yaml::Map(vec![
                                ("shell".to_owned(), Yaml::str("bash")),
                                ("run".to_owned(), Yaml::str("echo repeated")),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
    )]);
    let canonical = marker::with_marker(VERSION, &render_yaml(&document))?;
    let rendered =
        render_checked_workflow(".github/actions/sample/action.yml", &document, VERSION)?;
    assert_eq!(rendered, canonical);
    assert!(!rendered.contains("&r"));
    assert!(!rendered.contains("*r"));
    Ok(())
}

#[test]
fn over_cap_workflow_uses_marked_step_run_aliases() -> Result<(), RenderError> {
    let command = "x".repeat(260_000);
    let document = workflow(&command, 2);
    let canonical = marker::with_marker(VERSION, &render_yaml(&document))?;
    assert!(canonical.len() > MAX_WORKFLOW_BYTES);
    let rendered = render_checked_workflow(".github/workflows/ci.yml", &document, VERSION)?;
    assert!(rendered.starts_with(&marker::marker_for_version(VERSION)?));
    assert!(rendered.len() < MAX_WORKFLOW_BYTES);
    assert!(rendered.contains("run: &r1"));
    assert!(rendered.contains("run: *r1"));
    Ok(())
}

#[test]
fn irreducible_over_cap_workflow_still_fails_closed() {
    let command = "x".repeat(MAX_WORKFLOW_BYTES + 100);
    let document = workflow(&command, 1);
    let error = render_checked_workflow(".github/workflows/ci.yml", &document, VERSION)
        .expect_err("irreducible workflow must retain the production guard");
    assert!(matches!(error, RenderError::InvalidWorkflow(problem)
        if problem.starts_with("workflow_too_large:.github/workflows/ci.yml:")));
}

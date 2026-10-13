//! Prep overlap-then-join: concurrent branches, needs join, no step syntax.
use std::collections::BTreeMap;
use velnor_actions_workflow_jobs::overlap::{PrepOverlap, wire_prep_join};
use velnor_actions_workflow_steps::{RenderError, checkout_step};

use super::impl_renderer_fixtures::*;

fn shell(name: &str) -> Result<velnor_actions_contract_workflow::Step, RenderError> {
    scrubbed_shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    )
}

fn branch(
    id: &str,
    display: &str,
    step: &str,
) -> Result<(String, velnor_actions_contract_workflow::Job), RenderError> {
    Ok(job(
        id,
        display,
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, shell(step)?],
    ))
}

fn spec() -> PrepOverlap {
    PrepOverlap {
        download_job: "velnor-prep-download".to_owned(),
        image_job: "velnor-prep-image".to_owned(),
        join_job: "velnor-prep-join".to_owned(),
    }
}

#[test]
fn prep_overlap_joins_independent_branches() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        branch(
            "velnor-prep-download",
            "Download verified artifact",
            "Fetch asset",
        )?,
        branch("velnor-prep-image", "Prepare pinned image", "Pull image")?,
        branch("velnor-prep-join", "Consume prepared inputs", "Use inputs")?,
    ]);
    wire_prep_join(&mut jobs, &spec())?;
    assert!(jobs["velnor-prep-download"].needs.is_empty());
    assert!(jobs["velnor-prep-image"].needs.is_empty());
    assert_eq!(
        jobs["velnor-prep-join"].needs,
        ["velnor-prep-download", "velnor-prep-image"]
    );
    let text = strict(&fixture_ir(jobs.into_iter().collect()), &fixture_ctx())?;
    for token in ["parallel:", "background:"] {
        assert!(
            !text.contains(token),
            "v1 forbids step syntax {token}:\n{text}"
        );
    }
    let join_at = text.find("velnor-prep-join:").expect("join job");
    let window = snip(&text, join_at, 1600);
    assert!(
        window.contains("- velnor-prep-download"),
        "join needs:\n{window}"
    );
    assert!(
        window.contains("- velnor-prep-image"),
        "join needs:\n{window}"
    );
    Ok(())
}

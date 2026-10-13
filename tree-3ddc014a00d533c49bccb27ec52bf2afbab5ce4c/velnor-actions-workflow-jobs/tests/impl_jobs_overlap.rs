//! Prep overlap-then-join: concurrent branches, needs join, no step syntax.
use std::collections::BTreeMap;
use velnor_actions_workflow_jobs::overlap::{PrepOverlap, wire_prep_join};
use velnor_actions_workflow_steps::{RenderError, checkout_step};

use super::impl_jobs_fixtures::*;

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
fn prep_overlap_rejects_dependent_or_missing() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        branch(
            "velnor-prep-download",
            "Download verified artifact",
            "Fetch asset",
        )?,
        branch("velnor-prep-image", "Prepare pinned image", "Pull image")?,
        branch("velnor-prep-join", "Consume prepared inputs", "Use inputs")?,
    ]);
    let mut missing = spec();
    missing.image_job = "velnor-absent".to_owned();
    assert!(
        wire_prep_join(&mut jobs, &missing)
            .is_err_and(|err| format!("{err:?}").contains("prep_overlap_unknown_job")),
        "absent branch must fail"
    );
    let mut same = spec();
    same.image_job = same.download_job.clone();
    assert!(
        wire_prep_join(&mut jobs, &same)
            .is_err_and(|err| format!("{err:?}").contains("prep_overlap_not_distinct")),
        "shared branch must fail"
    );
    jobs.get_mut("velnor-prep-image")
        .expect("image job")
        .needs
        .push("velnor-prep-download".to_owned());
    assert!(
        wire_prep_join(&mut jobs, &spec())
            .is_err_and(|err| format!("{err:?}").contains("prep_overlap_dependent")),
        "ordered branches must fail"
    );
    jobs.get_mut("velnor-prep-image")
        .expect("image job")
        .needs
        .clear();
    jobs.get_mut("velnor-prep-download")
        .expect("download job")
        .needs
        .push("velnor-prep-join".to_owned());
    assert!(
        wire_prep_join(&mut jobs, &spec())
            .is_err_and(|err| format!("{err:?}").contains("prep_overlap_cycle")),
        "branch on join must fail"
    );
    Ok(())
}

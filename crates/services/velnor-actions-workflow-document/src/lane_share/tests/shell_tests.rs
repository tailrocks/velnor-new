use super::super::share_lanes;
use super::{ctx, echo_step, paired, render_jobs, workflow_ir};

#[test]
fn paired_consumer_workflow_sets_shell_for_each_typed_scale_lane_only() {
    let jobs = paired(&[echo_step(0, "paired-shell")]);
    let shared = share_lanes(&jobs, &ctx()).expect("paired lanes share");
    let yaml = render_jobs(&workflow_ir(), &shared, &ctx()).expect("consumer workflow renders");

    let scale_jobs = yaml
        .matches("runs-on: [velnor, ubuntu-26.04-scale-set]")
        .count();
    let hosted_jobs = yaml.matches("runs-on: ubuntu-26.04").count();
    assert_eq!(scale_jobs, 21);
    assert_eq!(hosted_jobs, 21);
    assert_eq!(yaml.matches("shell: bash -e {0}").count(), scale_jobs);
}

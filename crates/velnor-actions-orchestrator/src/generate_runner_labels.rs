//! Bridge only configured scale-set selectors and canonical generated hosted labels.
use std::collections::BTreeSet;
use velnor_actions_contract::{ReleaseTarget, ScaleSetSelector, VelnorConfig, WorkflowIr};

pub(super) fn collect(
    config: &VelnorConfig,
    workflow: &WorkflowIr,
    selector: Option<&ScaleSetSelector>,
) -> Vec<String> {
    let mut labels = selector
        .map(|value| value.labels().to_vec())
        .unwrap_or_default();
    let mut hosted = workflow
        .jobs
        .values()
        .map(|job| job.runs_on.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(release) = config.stacks.rust.as_ref().map(|rust| &rust.binary_release)
        && release.enabled()
    {
        hosted.insert(release.producer.runner.runs_on());
    }
    for label in hosted {
        if (ReleaseTarget::for_runner_label(label).is_some()
            || velnor_actions_contract::config::is_hosted_catalog(label))
            && !labels.iter().any(|existing| existing == label)
        {
            labels.push(label.to_owned());
        }
    }
    labels
}
